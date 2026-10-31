//! Mark viva: la cara de Atic que mira al cursor con todo el cuerpo, como la
//! de la app de Tauri (`src/lib/AticMark.svelte`, con sus mismos
//! números). El cuerpo (la «a» entera) gira hasta 12° y se inclina hacia el
//! cursor con un vaivén lento; los ojos se separan o se juntan con la
//! distancia, se angostan al mirar de lado y parpadean (sin ojeras). El cuerpo llega más
//! lento que la mirada, y quieta la animación se detiene (no pide cuadros).

use std::time::Instant;

use gpui::{point, px, Hsla, Window};

const MID_X: f32 = 12.0;
const MID_Y: f32 = 11.15;
const REST_SPREAD: f32 = 1.88;
const LOOK_MAX: f32 = 1.18;
const TURN_MAX: f32 = 12.0;
const RX_REST: f32 = 0.88;
const RY_REST: f32 = 1.32;
const JUNTO: f32 = 1.22;
const LEJOS: f32 = 2.05;
/// Lo que se inclina el cuerpo, en fracción del tamaño.
const LEAN: f32 = 0.05;
/// El cuadro de referencia de los factores de `lerp` (60 Hz).
const REF_FRAME_MS: f32 = 16.67;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Pose {
    look: (f32, f32),
    /// Grados.
    rot: f32,
    /// En fracción de `LEAN` del tamaño.
    lean: (f32, f32),
    spread: f32,
    rx: f32,
    ry: f32,
    /// 1 abiertos, ~0.1 cerrados.
    lid: f32,
}

impl Pose {
    pub(crate) const REST: Pose = Pose {
        look: (0.0, 0.0),
        rot: 0.0,
        lean: (0.0, 0.0),
        spread: REST_SPREAD,
        rx: RX_REST,
        ry: RY_REST,
        lid: 1.0,
    };
}

pub(crate) struct Face {
    now: Pose,
    want: Pose,
    last: Instant,
    /// La fase del vaivén: solo avanza mientras se anima.
    sway: f32,
    /// Lo que falta con los ojos cerrados, en ms.
    lid_hold: f32,
    next_blink: Instant,
    seed: u32,
}

impl Face {
    pub(crate) fn new() -> Self {
        let now = Instant::now();
        Self {
            now: Pose::REST,
            want: Pose::REST,
            last: now,
            sway: 0.0,
            lid_hold: 0.0,
            next_blink: now + std::time::Duration::from_millis(1800),
            seed: 0x9e37_79b9,
        }
    }

    /// Mira hacia `to` (el cursor menos el centro de la cara, en px), con la
    /// cara de `size` px de ancho.
    pub(crate) fn aim(&mut self, to: (f32, f32), size: f32) {
        let dist = (to.0 * to.0 + to.1 * to.1).sqrt().max(1.0);
        let face = size * 0.5;
        let reach = ((dist - face * 0.18) / (face * 1.35)).clamp(0.0, 1.0);
        let (nx, ny) = (to.0 / dist, to.1 / dist);
        self.want.look = (nx * LOOK_MAX * reach, ny * LOOK_MAX * reach);
        self.want.rot = nx * TURN_MAX * reach;
        self.want.lean = (nx * reach, ny * 0.4 * reach);
        self.want.spread = lerp(JUNTO, LEJOS, reach).clamp(JUNTO, LEJOS);
        self.want.rx = lerp(RX_REST + 0.1, RX_REST * 0.72, reach * nx.abs());
        self.want.ry = lerp(RY_REST + 0.08, RY_REST * 0.92, reach);
    }

    fn random(&mut self) -> f32 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 17;
        self.seed ^= self.seed << 5;
        (self.seed % 10_000) as f32 / 10_000.0
    }

    /// Avanza hasta `now`. Devuelve si sigue en movimiento (pedir otro cuadro).
    pub(crate) fn step(&mut self, now: Instant) -> bool {
        let dt = (now.duration_since(self.last).as_secs_f32() * 1000.0).min(50.0);
        self.last = now;
        if now >= self.next_blink {
            self.want.lid = 0.1;
            self.lid_hold = 60.0 + self.random() * 50.0;
            let wait = 2600.0 + self.random() * 6400.0;
            self.next_blink = now + std::time::Duration::from_millis(wait as u64);
        }
        if self.lid_hold > 0.0 {
            self.lid_hold -= dt;
            if self.lid_hold <= 0.0 {
                self.want.lid = 1.0;
            }
        }
        let k = |f: f32| 1.0 - (1.0 - f).powf(dt / REF_FRAME_MS);
        let (body, gaze, shape) = (k(0.08), k(0.16), k(0.14));
        let (n, w) = (&mut self.now, self.want);
        n.rot += (w.rot - n.rot) * body;
        n.lean.0 += (w.lean.0 - n.lean.0) * body;
        n.lean.1 += (w.lean.1 - n.lean.1) * body;
        n.look.0 += (w.look.0 - n.look.0) * gaze;
        n.look.1 += (w.look.1 - n.look.1) * gaze;
        n.spread = (n.spread + (w.spread - n.spread) * shape).clamp(JUNTO, LEJOS);
        n.rx += (w.rx - n.rx) * shape;
        n.ry += (w.ry - n.ry) * shape;
        n.lid += (w.lid - n.lid) * k(0.42);
        let moving = self.lid_hold > 0.0
            || (w.rot - n.rot).abs() > 0.01
            || (w.lean.0 - n.lean.0).abs() > 0.002
            || (w.lean.1 - n.lean.1).abs() > 0.002
            || (w.look.0 - n.look.0).abs() > 0.01
            || (w.look.1 - n.look.1).abs() > 0.01
            || (w.spread - n.spread).abs() > 0.01
            || (w.rx - n.rx).abs() > 0.005
            || (w.ry - n.ry).abs() > 0.005
            || (w.lid - n.lid).abs() > 0.005;
        if moving {
            self.sway += dt;
        }
        moving
    }

    /// Lo que se dibuja ahora, con el vaivén.
    pub(crate) fn pose(&self) -> Pose {
        Pose { rot: self.now.rot + (self.sway / 1100.0).sin() * 1.2, ..self.now }
    }

    /// Cuándo hay que volver a mirar aunque nada se mueva: el próximo parpadeo.
    pub(crate) fn next_blink(&self) -> Instant {
        self.next_blink
    }
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Cuánto se sale un ojo del círculo (la cara), para devolverlo adentro.
fn overflow(x: f32, y: f32, pad: f32) -> (f32, f32) {
    let (dx, dy) = (x - 12.0, y - 12.0);
    let limit = 4.55 - pad;
    let d = (dx * dx + dy * dy).sqrt().max(1e-3);
    if d <= limit {
        return (0.0, 0.0);
    }
    let extra = (d - limit) / d;
    (dx * extra, dy * extra)
}

/// Los dos ojos en la retícula de 24, sin salirse de la cara.
fn place_pair(pose: &Pose, ry: f32) -> [(f32, f32); 2] {
    let pad = pose.rx.max(ry);
    let y = MID_Y + pose.look.1;
    let left = MID_X + pose.look.0 - pose.spread;
    let right = MID_X + pose.look.0 + pose.spread;
    let ol = overflow(left, y, pad);
    let or = overflow(right, y, pad);
    let ox = if ol.0.abs() > or.0.abs() { ol.0 } else { or.0 };
    let oy = if ol.1.abs() > or.1.abs() { ol.1 } else { or.1 };
    [(left - ox, y - oy), (right - ox, y - oy)]
}

/// Dibuja la cara en `center`, de `size` px, con la pose dada.
pub(crate) fn paint(window: &mut Window, center: (f32, f32), size: f32, pose: Pose, color: Hsla) {
    let unit = size / 24.0;
    let stroke = 1.5 * unit;
    let (sin, cos) = pose.rot.to_radians().sin_cos();
    let lean = (pose.lean.0 * size * LEAN, pose.lean.1 * size * LEAN);
    // De la retícula de 24 (centro 12,12) a la pantalla, girada e inclinada.
    let at = |gx: f32, gy: f32| {
        let (x, y) = ((gx - 12.0) * unit, (gy - 12.0) * unit);
        (center.0 + lean.0 + x * cos - y * sin, center.1 + lean.1 + x * sin + y * cos)
    };
    let line = |window: &mut Window, from: (f32, f32), to: (f32, f32), width: f32| {
        let mut path = gpui::PathBuilder::stroke(px(width));
        path.move_to(point(px(from.0), px(from.1)));
        path.line_to(point(px(to.0), px(to.1)));
        if let Ok(path) = path.build() {
            window.paint_path(path, color);
        }
    };

    // La cabeza: el círculo y el palo de la «a».
    let (cx, cy) = at(12.0, 12.0);
    if let Some(ring) = crate::ring_path(cx, cy, 5.5 * unit, stroke) {
        window.paint_path(ring, color);
    }
    line(window, at(17.5, 6.5), at(17.5, 17.5), stroke);

    // Los ojos.
    let ry = (pose.ry * pose.lid).max(0.08);
    for (ex, ey) in place_pair(&pose, ry) {
        let (x, y) = at(ex, ey);
        if let Some(eye) = crate::liquid::ellipse(x, y, pose.rx * unit, ry * unit) {
            window.paint_path(eye, color);
        }
    }
}

/// La pose para una mirada ya suavizada (la de la pill: `eyes` de largo
/// hasta 1, con sus vistazos al azar) y su parpadeo (`blink`, 1 abiertos).
pub(crate) fn pose_for(eyes: (f32, f32), blink: f32) -> Pose {
    let reach = (eyes.0 * eyes.0 + eyes.1 * eyes.1).sqrt().min(1.0);
    let (nx, ny) = if reach > 1e-3 { (eyes.0 / reach.max(1e-3), eyes.1 / reach.max(1e-3)) } else { (0.0, 0.0) };
    let (nx, ny) = (nx.clamp(-1.0, 1.0), ny.clamp(-1.0, 1.0));
    Pose {
        look: (nx * LOOK_MAX * reach, ny * LOOK_MAX * reach),
        rot: nx * TURN_MAX * reach,
        lean: (nx * reach, ny * 0.4 * reach),
        spread: lerp(JUNTO, LEJOS, reach).clamp(JUNTO, LEJOS),
        rx: lerp(RX_REST + 0.1, RX_REST * 0.72, reach * nx.abs()),
        ry: lerp(RY_REST + 0.08, RY_REST * 0.92, reach),
        lid: blink.clamp(0.08, 1.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mira_hacia_el_cursor_y_gira_hacia_ese_lado() {
        let mut face = Face::new();
        face.aim((300.0, 0.0), 80.0);
        assert!(face.want.look.0 > 1.0 && face.want.rot > 11.0);
        face.aim((-300.0, 0.0), 80.0);
        assert!(face.want.look.0 < -1.0 && face.want.rot < -11.0);
        // Encima de la cara no gira.
        face.aim((2.0, 0.0), 80.0);
        assert!(face.want.rot.abs() < 0.01);
    }

    #[test]
    fn quieta_deja_de_pedir_cuadros() {
        let mut face = Face::new();
        face.next_blink = Instant::now() + std::time::Duration::from_secs(60);
        face.aim((200.0, 50.0), 80.0);
        let mut at = Instant::now();
        let mut moving = true;
        for _ in 0..600 {
            at += std::time::Duration::from_millis(16);
            moving = face.step(at);
            if !moving {
                break;
            }
        }
        assert!(!moving, "la cara se asienta");
    }

    #[test]
    fn la_mirada_de_la_pill_gira_el_cuerpo() {
        assert!(pose_for((1.0, 0.0), 1.0).rot > 11.0);
        assert!(pose_for((-0.5, 0.0), 1.0).rot < -5.0);
        assert_eq!(pose_for((0.0, 0.0), 1.0).rot, 0.0);
    }

    #[test]
    fn los_ojos_no_se_salen_de_la_cara() {
        let pose = Pose { look: (3.0, 3.0), spread: LEJOS, ..Pose::REST };
        for (x, y) in place_pair(&pose, pose.ry) {
            let d = ((x - 12.0).powi(2) + (y - 12.0).powi(2)).sqrt();
            assert!(d <= 4.55, "ojo en {x},{y}");
        }
    }
}
