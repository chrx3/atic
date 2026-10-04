//! Geometría del acoplado: bordes, área de trabajo y dónde va la pill.
//! Reglas de `edgeDock.ts`; todo en píxeles lógicos de la
//! ventana del overlay, que cubre el monitor principal.

/// Lo que hay que alejarse del borde para soltar el tab (`DOCK_RELEASE_PX`).
pub const UNDOCK_DISTANCE: f32 = 64.0;
/// Distancia al borde a la que la gota se acopla al soltarla (`DOCK_SNAP_PX`).
pub const DOCK_SNAP: f32 = 28.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    pub fn centered((cx, cy): (f32, f32), w: f32, h: f32) -> Self {
        Self::new(cx - w / 2.0, cy - h / 2.0, w, h)
    }

    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }

    pub fn center(&self) -> (f32, f32) {
        (self.x + self.w / 2.0, self.y + self.h / 2.0)
    }

    pub fn contains(&self, (x, y): (f32, f32), margin: f32) -> bool {
        x >= self.x - margin
            && x <= self.right() + margin
            && y >= self.y - margin
            && y <= self.bottom() + margin
    }

    /// Mueve el rectángulo lo mínimo para que quede dentro de `bounds`.
    pub fn clamped_into(&self, bounds: &Rect) -> Self {
        let x = self.x.min(bounds.right() - self.w).max(bounds.x);
        let y = self.y.min(bounds.bottom() - self.h).max(bounds.y);
        Self::new(x, y, self.w, self.h)
    }
}

/// Borde de la pantalla, o lado hacia el que se abre algo respecto de la pill.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    Top,
    Bottom,
    Left,
    Right,
}

impl Edge {
    /// En los bordes izquierdo y derecho el tab va de pie.
    pub fn is_vertical(self) -> bool {
        matches!(self, Edge::Left | Edge::Right)
    }

    /// Punto de pantalla a `along` sobre el borde y `depth` hacia adentro.
    pub fn point(self, work: &Rect, along: f32, depth: f32) -> (f32, f32) {
        match self {
            Edge::Top => (along, work.y + depth),
            Edge::Bottom => (along, work.bottom() - depth),
            Edge::Left => (work.x + depth, along),
            Edge::Right => (work.right() - depth, along),
        }
    }

    /// Coordenada a lo largo del borde de un punto de pantalla.
    pub fn along_of(self, (x, y): (f32, f32)) -> f32 {
        if self.is_vertical() {
            y
        } else {
            x
        }
    }

    /// Cuánto se metió un punto hacia el interior desde este borde.
    pub fn depth_of(self, work: &Rect, (x, y): (f32, f32)) -> f32 {
        match self {
            Edge::Top => y - work.y,
            Edge::Bottom => work.bottom() - y,
            Edge::Left => x - work.x,
            Edge::Right => work.right() - x,
        }
    }

    /// Tramo del área de trabajo que recorre este borde.
    pub fn span(self, work: &Rect) -> (f32, f32) {
        if self.is_vertical() {
            (work.y, work.bottom())
        } else {
            (work.x, work.right())
        }
    }

    /// Rectángulo de un tab de largo `length` y grosor `thick` centrado en
    /// `along`.
    pub fn tab_rect(self, work: &Rect, along: f32, length: f32, thick: f32) -> Rect {
        let start = along - length / 2.0;
        match self {
            Edge::Top => Rect::new(start, work.y, length, thick),
            Edge::Bottom => Rect::new(start, work.bottom() - thick, length, thick),
            Edge::Left => Rect::new(work.x, start, thick, length),
            Edge::Right => Rect::new(work.right() - thick, start, thick, length),
        }
    }
}

/// Bordes donde se puede acoplar: los del monitor que no tienen la barra de
/// tareas (Atic descarta los que el sistema reservó más de 2 px).
pub fn dockable_edges(monitor: &Rect, work: &Rect) -> Vec<Edge> {
    let reserved = |a: f32, b: f32| (a - b).abs() > 2.0;
    let mut edges = Vec::new();
    if !reserved(work.x, monitor.x) {
        edges.push(Edge::Left);
    }
    if !reserved(work.right(), monitor.right()) {
        edges.push(Edge::Right);
    }
    if !reserved(work.y, monitor.y) {
        edges.push(Edge::Top);
    }
    if !reserved(work.bottom(), monitor.bottom()) {
        edges.push(Edge::Bottom);
    }
    edges
}

/// Al soltar la gota: el borde más cercano a `DOCK_SNAP` o menos. En una
/// esquina ganan izquierda y derecha, porque se revisan primero y el empate
/// no reemplaza (`dockCandidate`).
pub fn dock_candidate(work: &Rect, disc: &Rect, dockable: &[Edge]) -> Option<Edge> {
    let gaps = [
        (Edge::Left, disc.x - work.x),
        (Edge::Right, work.right() - disc.right()),
        (Edge::Top, disc.y - work.y),
        (Edge::Bottom, work.bottom() - disc.bottom()),
    ];
    let mut best: Option<(Edge, f32)> = None;
    for (edge, gap) in gaps {
        let gap = gap.max(0.0);
        if gap > DOCK_SNAP || !dockable.contains(&edge) {
            continue;
        }
        if best.is_none_or(|(_, best_gap)| gap < best_gap) {
            best = Some((edge, gap));
        }
    }
    best.map(|(edge, _)| edge)
}

/// Dónde queda el tab a lo largo del borde. Arriba siempre al centro (como el
/// notch); en los demás bordes conserva la posición, dentro del tramo.
pub fn dock_along(edge: Edge, work: &Rect, center: (f32, f32), length: f32) -> f32 {
    let (start, end) = edge.span(work);
    if edge == Edge::Top {
        return (start + end) / 2.0;
    }
    clamp_along(edge, work, edge.along_of(center), length)
}

pub fn clamp_along(edge: Edge, work: &Rect, along: f32, length: f32) -> f32 {
    let (start, end) = edge.span(work);
    along.clamp(
        start + length / 2.0,
        (end - length / 2.0).max(start + length / 2.0),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const MONITOR: Rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 1920.0,
        h: 1080.0,
    };
    // Barra de tareas abajo, de 48 px.
    const WORK: Rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 1920.0,
        h: 1032.0,
    };

    fn disc_at(cx: f32, cy: f32) -> Rect {
        Rect::centered((cx, cy), 52.0, 52.0)
    }

    #[test]
    fn el_borde_de_la_barra_de_tareas_no_es_acoplable() {
        let edges = dockable_edges(&MONITOR, &WORK);
        assert!(!edges.contains(&Edge::Bottom));
        assert!(edges.contains(&Edge::Top) && edges.contains(&Edge::Left));
    }

    #[test]
    fn se_acopla_solo_cerca_del_borde() {
        let edges = dockable_edges(&MONITOR, &WORK);
        assert_eq!(
            dock_candidate(&WORK, &disc_at(500.0, 26.0 + 28.0), &edges),
            Some(Edge::Top)
        );
        assert_eq!(
            dock_candidate(&WORK, &disc_at(500.0, 26.0 + 29.0), &edges),
            None
        );
        // Cerca de la barra de tareas no se acopla abajo.
        assert_eq!(
            dock_candidate(&WORK, &disc_at(500.0, 1032.0 - 30.0), &edges),
            None
        );
    }

    #[test]
    fn en_la_esquina_gana_el_lateral() {
        let edges = dockable_edges(&MONITOR, &WORK);
        assert_eq!(
            dock_candidate(&WORK, &disc_at(26.0, 26.0), &edges),
            Some(Edge::Left)
        );
    }

    #[test]
    fn arriba_va_al_centro_y_al_costado_conserva_la_altura() {
        assert_eq!(dock_along(Edge::Top, &WORK, (300.0, 20.0), 124.0), 960.0);
        assert_eq!(dock_along(Edge::Left, &WORK, (20.0, 400.0), 124.0), 400.0);
        // Pegado a la esquina, el tab no se sale del tramo.
        assert_eq!(dock_along(Edge::Right, &WORK, (1900.0, 10.0), 124.0), 62.0);
    }
}
