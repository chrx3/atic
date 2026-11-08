//! La geometría de las pantallas de un equipo: dónde está su borde hacia el
//! otro, por dónde entra el cursor y dónde se lo devuelve.
//!
//! Las coordenadas son las del cursor del sistema: en macOS, las globales de
//! `CGEvent` (origen arriba a la izquierda de la pantalla principal); en
//! Windows, las del escritorio virtual en píxeles físicos. En los dos, `y`
//! crece hacia abajo y hay pantallas con coordenadas negativas.
//!
//! El punto del borde viaja como fracción (`along`, de 0 a 1) del largo total
//! de ese lado: el cursor sale por la mitad del borde derecho del PC y entra
//! por la mitad del borde izquierdo del Mac, aunque las pantallas no midan lo
//! mismo.

use serde::{Deserialize, Serialize};

/// De qué lado de este equipo está el otro.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    Left,
    Right,
    Top,
    Bottom,
}

impl Side {
    pub fn opposite(self) -> Self {
        match self {
            Side::Left => Side::Right,
            Side::Right => Side::Left,
            Side::Top => Side::Bottom,
            Side::Bottom => Side::Top,
        }
    }

    /// Cuánto empuja un movimiento hacia fuera por este lado (negativo: hacia dentro).
    pub fn outward(self, dx: f64, dy: f64) -> f64 {
        match self {
            Side::Left => -dx,
            Side::Right => dx,
            Side::Top => -dy,
            Side::Bottom => dy,
        }
    }

    /// Los lados izquierdo y derecho se recorren de arriba abajo (`y`).
    fn vertical_edge(self) -> bool {
        matches!(self, Side::Left | Side::Right)
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "left" | "izquierda" | "izq" => Some(Side::Left),
            "right" | "derecha" | "der" => Some(Side::Right),
            "top" | "up" | "arriba" => Some(Side::Top),
            "bottom" | "down" | "abajo" => Some(Side::Bottom),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    fn right(&self) -> f64 {
        self.x + self.w - 1.0
    }

    fn bottom(&self) -> f64 {
        self.y + self.h - 1.0
    }

    fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }

    fn clamp(&self, x: f64, y: f64) -> (f64, f64) {
        (x.clamp(self.x, self.right()), y.clamp(self.y, self.bottom()))
    }

    /// El tramo del borde (`desde`, `hasta`) por el eje que recorre ese lado.
    fn span(&self, side: Side) -> (f64, f64) {
        if side.vertical_edge() {
            (self.y, self.bottom())
        } else {
            (self.x, self.right())
        }
    }

    /// La coordenada del borde de ese lado (la `x` de la izquierda o la derecha, la `y` de arriba o abajo).
    fn edge(&self, side: Side) -> f64 {
        match side {
            Side::Left => self.x,
            Side::Right => self.right(),
            Side::Top => self.y,
            Side::Bottom => self.bottom(),
        }
    }
}

/// Las pantallas activas de un equipo.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Screens(pub Vec<Rect>);

impl Screens {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// El rectángulo que las envuelve a todas.
    pub fn bounds(&self) -> Option<Rect> {
        let first = self.0.first()?;
        let (mut x0, mut y0, mut x1, mut y1) = (first.x, first.y, first.right(), first.bottom());
        for r in &self.0[1..] {
            x0 = x0.min(r.x);
            y0 = y0.min(r.y);
            x1 = x1.max(r.right());
            y1 = y1.max(r.bottom());
        }
        Some(Rect { x: x0, y: y0, w: x1 - x0 + 1.0, h: y1 - y0 + 1.0 })
    }

    /// La pantalla donde está el punto, o la más cercana.
    fn display_near(&self, x: f64, y: f64) -> Option<&Rect> {
        self.0.iter().find(|r| r.contains(x, y)).or_else(|| {
            self.0.iter().min_by(|a, b| {
                let da = dist2(a.clamp(x, y), (x, y));
                let db = dist2(b.clamp(x, y), (x, y));
                da.total_cmp(&db)
            })
        })
    }

    /// El punto más cercano dentro de alguna pantalla: fuera de todas, el cursor se pierde.
    pub fn clamp(&self, x: f64, y: f64) -> (f64, f64) {
        match self.display_near(x, y) {
            Some(r) => r.clamp(x, y),
            None => (x, y),
        }
    }

    /// Si el punto está en el borde `side` de su pantalla y del otro lado no
    /// hay otra pantalla de este equipo, a qué fracción del largo de ese lado.
    pub fn at_edge(&self, side: Side, x: f64, y: f64) -> Option<f64> {
        let r = self.display_near(x, y)?;
        let (cx, cy) = r.clamp(x, y);
        let on_border = match side {
            Side::Left | Side::Top => (if side.vertical_edge() { cx } else { cy }) <= r.edge(side) + 0.5,
            Side::Right | Side::Bottom => (if side.vertical_edge() { cx } else { cy }) >= r.edge(side) - 0.5,
        };
        if !on_border {
            return None;
        }
        // Un poco más allá del borde: si cae en otra pantalla, el cursor sigue en este equipo.
        let (bx, by) = match side {
            Side::Left => (cx - 1.5, cy),
            Side::Right => (cx + 1.5, cy),
            Side::Top => (cx, cy - 1.5),
            Side::Bottom => (cx, cy + 1.5),
        };
        if self.0.iter().any(|o| o.contains(bx, by)) {
            return None;
        }
        Some(self.along(side, cx, cy))
    }

    /// La fracción del punto a lo largo del lado `side` del conjunto.
    fn along(&self, side: Side, x: f64, y: f64) -> f64 {
        let Some(b) = self.bounds() else { return 0.5 };
        let (lo, hi) = b.span(side);
        let c = if side.vertical_edge() { y } else { x };
        if hi <= lo {
            return 0.5;
        }
        ((c - lo) / (hi - lo)).clamp(0.0, 1.0)
    }

    /// Dónde aparece el cursor que entra por `side` a `along` de su largo: en
    /// el borde expuesto de la pantalla que está a esa altura (o la más cercana).
    pub fn entry(&self, side: Side, along: f64) -> Option<(f64, f64)> {
        let b = self.bounds()?;
        let (lo, hi) = b.span(side);
        let c = lo + along.clamp(0.0, 1.0) * (hi - lo);
        // Las pantallas cuyo tramo cubre `c`; de ellas, la que está más hacia fuera.
        let outermost = |a: &&Rect, z: &&Rect| match side {
            Side::Left | Side::Top => z.edge(side).total_cmp(&a.edge(side)),
            Side::Right | Side::Bottom => a.edge(side).total_cmp(&z.edge(side)),
        };
        let covering = self.0.iter().filter(|r| {
            let (a, z) = r.span(side);
            c >= a && c <= z
        });
        let r = match covering.max_by(outermost) {
            Some(r) => r,
            None => self.0.iter().min_by(|a, z| span_dist(a.span(side), c).total_cmp(&span_dist(z.span(side), c)))?,
        };
        let (a, z) = r.span(side);
        let c = c.clamp(a, z);
        Some(if side.vertical_edge() { (r.edge(side), c) } else { (c, r.edge(side)) })
    }

    /// El punto de entrada movido `by` píxeles hacia dentro de su pantalla:
    /// donde se deja el cursor al devolverlo, para que no vuelva a cruzar solo.
    pub fn inward(&self, side: Side, along: f64, by: f64) -> Option<(f64, f64)> {
        let (x, y) = self.entry(side, along)?;
        let (x, y) = match side {
            Side::Left => (x + by, y),
            Side::Right => (x - by, y),
            Side::Top => (x, y + by),
            Side::Bottom => (x, y - by),
        };
        Some(self.clamp(x, y))
    }
}

fn dist2(a: (f64, f64), b: (f64, f64)) -> f64 {
    (a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)
}

fn span_dist((a, z): (f64, f64), c: f64) -> f64 {
    if c < a {
        a - c
    } else if c > z {
        c - z
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(x: f64, y: f64, w: f64, h: f64) -> Rect {
        Rect { x, y, w, h }
    }

    /// Un PC con dos monitores lado a lado: 1920x1080 y, a su derecha, 2560x1440 más arriba.
    fn pc() -> Screens {
        Screens(vec![r(0., 0., 1920., 1080.), r(1920., -360., 2560., 1440.)])
    }

    #[test]
    fn el_borde_entre_dos_monitores_no_cuenta() {
        let s = pc();
        // El borde derecho del primero tiene al segundo pegado.
        assert_eq!(s.at_edge(Side::Right, 1919., 500.), None);
        // El borde derecho del segundo sí.
        assert!(s.at_edge(Side::Right, 4479., 500.).is_some());
        // Lejos del borde, no.
        assert_eq!(s.at_edge(Side::Right, 4000., 500.), None);
    }

    #[test]
    fn un_punto_mas_alla_del_borde_cuenta_como_borde() {
        // El gancho de Windows puede dar el punto sin recortar a la pantalla.
        let s = pc();
        assert!(s.at_edge(Side::Right, 4485., 0.).is_some());
    }

    #[test]
    fn la_fraccion_se_mide_sobre_todo_el_lado() {
        let s = pc();
        // El conjunto va de y = -360 a 1079.
        let along = s.at_edge(Side::Right, 4479., -360.).unwrap();
        assert!(along.abs() < 1e-9);
        let along = s.at_edge(Side::Right, 4479., 1079.).unwrap();
        assert!((along - 1.0).abs() < 1e-9);
    }

    #[test]
    fn se_entra_por_el_borde_expuesto_de_la_pantalla_a_esa_altura() {
        // Un Mac con su pantalla y un monitor a la izquierda, más bajo.
        let mac = Screens(vec![r(0., 0., 1512., 982.), r(-1920., 200., 1920., 1080.)]);
        // Entrando por la izquierda, arriba del todo: solo el Mac cubre esa altura.
        let (x, y) = mac.entry(Side::Left, 0.0).unwrap();
        assert_eq!((x, y), (0., 0.));
        // Más abajo, el monitor está más hacia fuera.
        let (x, _) = mac.entry(Side::Left, 0.8).unwrap();
        assert_eq!(x, -1920.);
        // Por la derecha, arriba: el Mac. Abajo del todo el Mac ya no llega, y el
        // borde expuesto a esa altura es el derecho del monitor.
        assert_eq!(mac.entry(Side::Right, 0.0).unwrap(), (1511., 0.));
        assert_eq!(mac.entry(Side::Right, 1.0).unwrap(), (-1., 1279.));
    }

    #[test]
    fn salir_y_volver_cae_en_el_mismo_punto() {
        let s = pc();
        let along = s.at_edge(Side::Right, 4479., 700.).unwrap();
        let (x, y) = s.entry(Side::Right, along).unwrap();
        assert_eq!((x, y), (4479., 700.));
        let (x, _) = s.inward(Side::Right, along, 10.).unwrap();
        assert_eq!(x, 4469.);
        assert_eq!(s.at_edge(Side::Right, x, y), None);
    }

    #[test]
    fn hacia_fuera_depende_del_lado() {
        assert!(Side::Right.outward(3., 0.) > 0.);
        assert!(Side::Left.outward(3., 0.) < 0.);
        assert!(Side::Top.outward(0., -2.) > 0.);
        assert_eq!(Side::Bottom.opposite(), Side::Top);
    }
}
