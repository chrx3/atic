//! Geometría del acoplado: bordes, área de trabajo y dónde va la pill.
//! Reglas de `edgeDock.ts`; todo en píxeles lógicos de la
//! ventana del overlay, que cubre el monitor donde está la pill.

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

    /// Un rectángulo de pie de `w × h` medido como tab: (largo a lo largo del
    /// borde, grosor hacia adentro). Los paneles y los vistazos no se giran
    /// con el borde: el texto va siempre derecho, así que en un costado su
    /// ancho es grosor y su alto es largo.
    pub fn extent(self, w: f32, h: f32) -> (f32, f32) {
        if self.is_vertical() {
            (h, w)
        } else {
            (w, h)
        }
    }

    /// Lo que queda de `rect` al quitarle la franja de `band` pegada al borde.
    pub fn beyond_band(self, rect: &Rect, band: f32) -> Rect {
        match self {
            Edge::Top => Rect::new(rect.x, rect.y + band, rect.w, rect.h - band),
            Edge::Bottom => Rect::new(rect.x, rect.y, rect.w, rect.h - band),
            Edge::Left => Rect::new(rect.x + band, rect.y, rect.w - band, rect.h),
            Edge::Right => Rect::new(rect.x, rect.y, rect.w - band, rect.h),
        }
    }

}

/// El notch abierto en `edge`: el panel de pie de `w × h`, pegado al borde,
/// centrado en `along` y corrido lo justo para no salirse del área de trabajo.
pub fn notch_rect(edge: Edge, work: &Rect, along: f32, w: f32, h: f32) -> Rect {
    let (length, thick) = edge.extent(w, h);
    edge.tab_rect(work, clamp_along(edge, work, along, length), length, thick)
}

/// El notch se abre en su lugar si cabe ahí un panel de `w` de ancho y al
/// menos `min_h` de alto; si no (un monitor muy bajo, con la pill a un
/// costado), la pill vuela arriba. Arriba cabe siempre: es el notch de siempre.
pub fn notch_fits(edge: Edge, work: &Rect, w: f32, min_h: f32) -> bool {
    if edge == Edge::Top {
        return true;
    }
    let (length, thick) = edge.extent(w, min_h);
    let (start, end) = edge.span(work);
    let depth = if edge.is_vertical() { work.w } else { work.h };
    end - start >= length && depth >= thick
}

/// El bloque que sale al lado de la columna en un costado (los vistazos, el
/// aviso, la letra, el dictado): `w × h`, pegado a la franja de grosor `band`,
/// centrado en `anchor` (la altura de la herramienta bajo el cursor, o la de la
/// marca) y corrido lo justo para no salirse del área de trabajo.
pub fn side_block(edge: Edge, work: &Rect, band: f32, anchor: f32, w: f32, h: f32) -> Rect {
    let x = match edge {
        Edge::Right => work.right() - band - w,
        _ => work.x + band,
    };
    let y = (anchor - h / 2.0).min(work.bottom() - h).max(work.y);
    Rect::new(x, y, w, h)
}

/// Una esquina redondeada de 90° como polilínea: `vertex` es la esquina viva,
/// `din` la dirección con que llega el contorno y `dout` con la que sale. Vale
/// igual para una esquina convexa que para una cóncava (el centro del arco
/// queda del lado de adentro de la vuelta).
fn corner(out: &mut Vec<(f32, f32)>, vertex: (f32, f32), din: (f32, f32), dout: (f32, f32), r: f32) {
    if r < 0.5 {
        out.push(vertex);
        return;
    }
    let center = (vertex.0 + (dout.0 - din.0) * r, vertex.1 + (dout.1 - din.1) * r);
    const STEPS: usize = 10;
    for k in 0..=STEPS {
        let a = k as f32 / STEPS as f32 * std::f32::consts::FRAC_PI_2;
        let (c, s) = (a.cos(), a.sin());
        out.push((
            center.0 + r * (-dout.0 * c + din.0 * s),
            center.1 + r * (-dout.1 * c + din.1 * s),
        ));
    }
}

/// El contorno del tab de un costado con su bloque: la columna de la tira y el
/// bloque al lado, una sola forma. Las esquinas que miran al escritorio son
/// redondas (`r`) y donde el bloque se une a la columna la piel hace una
/// esquina cóncava (`rc`), como una gota que se estira. Cuando los bordes de
/// los dos casi coinciden, los radios se achican con la distancia: no hay
/// saltos al deslizarse el bloque. El lado del borde de la pantalla se pasa
/// 1 px (como `grow_outward`) para que el antialias no deje una línea.
///
/// Se arma en coordenadas del borde (hacia adentro, a lo largo) y se pasa a
/// pantalla: a la derecha queda espejado.
pub fn side_outline(edge: Edge, work: &Rect, column: &Rect, block: &Rect, r: f32, rc: f32) -> Vec<(f32, f32)> {
    let t = edge.depth_of(work, column.center()) * 2.0;
    let w = block.w;
    let (c0, c1) = (column.y, column.bottom());
    let (b0, b1) = (block.y, block.bottom());
    // Radio de una esquina de la columna y uno del bloque, según el lugar.
    let rcol = r.min(t / 2.0).min((c1 - c0) / 2.0);
    let rblk = r.min(w / 2.0).min((b1 - b0) / 2.0);
    // Dos esquinas seguidas en un tramo de largo `gap`: se reparten el tramo.
    let split = |gap: f32, a: f32, b: f32| -> (f32, f32) {
        let gap = gap.max(0.0);
        if a + b <= gap {
            (a, b)
        } else {
            let k = gap / (a + b).max(0.001);
            (a * k, b * k)
        }
    };
    let (right, down, left, up) = ((1.0, 0.0), (0.0, 1.0), (-1.0, 0.0), (0.0, -1.0));
    let mut pts: Vec<(f32, f32)> = Vec::new();
    pts.push((-1.0, c0));
    // Arriba.
    let top = b0 - c0;
    if top > 0.5 {
        // El bloque empieza más abajo: la columna dobla y baja hasta él.
        let (a, b) = split(top, rcol, rc.min(w / 2.0));
        corner(&mut pts, (t, c0), right, down, a);
        corner(&mut pts, (t, b0), down, right, b);
    } else if top < -0.5 {
        // El bloque sube más que la columna: se sube por su costado.
        let (a, b) = split(-top, rc.min(t), rblk.min(-top));
        corner(&mut pts, (t, c0), right, up, a);
        corner(&mut pts, (t, b0), up, right, b);
    }
    corner(&mut pts, (t + w, b0), right, down, rblk);
    corner(&mut pts, (t + w, b1), down, left, rblk);
    // Abajo.
    let bottom = c1 - b1;
    if bottom > 0.5 {
        let (a, b) = split(bottom, rc.min(w / 2.0), rcol);
        corner(&mut pts, (t, b1), left, down, a);
        corner(&mut pts, (t, c1), down, left, b);
    } else if bottom < -0.5 {
        let (a, b) = split(-bottom, rblk.min(-bottom), rc.min(t));
        corner(&mut pts, (t, b1), left, up, a);
        corner(&mut pts, (t, c1), up, left, b);
    }
    pts.push((-1.0, c1));
    pts.into_iter()
        .map(|(u, v)| match edge {
            Edge::Right => (work.right() - u, v),
            _ => (work.x + u, v),
        })
        .collect()
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

/// Bordes de `monitor` que dan al vacío: los que no tienen otro monitor
/// pegado (`isOuterEdge` en Atic). Con dos pantallas lado a lado, el canto
/// entre ellas es la mitad del escritorio, no un borde donde acoplar. Todo en
/// píxeles físicos, para que escalas distintas no descuadren los cantos.
pub fn outer_edges(monitor: &Rect, all: &[Rect]) -> Vec<Edge> {
    const TOLERANCE: f32 = 1.0;
    let touches = |a: f32, b: f32| (a - b).abs() <= TOLERANCE;
    let overlaps = |a0: f32, a1: f32, b0: f32, b1: f32| a1.min(b1) - a0.max(b0) > TOLERANCE;
    let blocked = |edge: Edge| {
        all.iter().filter(|other| *other != monitor).any(|other| match edge {
            Edge::Left => {
                touches(other.right(), monitor.x)
                    && overlaps(monitor.y, monitor.bottom(), other.y, other.bottom())
            }
            Edge::Right => {
                touches(other.x, monitor.right())
                    && overlaps(monitor.y, monitor.bottom(), other.y, other.bottom())
            }
            Edge::Top => {
                touches(other.bottom(), monitor.y)
                    && overlaps(monitor.x, monitor.right(), other.x, other.right())
            }
            Edge::Bottom => {
                touches(other.y, monitor.bottom())
                    && overlaps(monitor.x, monitor.right(), other.x, other.right())
            }
        })
    };
    [Edge::Left, Edge::Right, Edge::Top, Edge::Bottom]
        .into_iter()
        .filter(|&edge| !blocked(edge))
        .collect()
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
    fn el_canto_entre_dos_pantallas_no_es_exterior() {
        // El notebook a la izquierda (1920 físicos al 125 %) y el principal.
        let laptop = Rect::new(-1920.0, 0.0, 1920.0, 1080.0);
        let main = Rect::new(0.0, 0.0, 1920.0, 1080.0);
        let all = [laptop, main];
        assert_eq!(outer_edges(&main, &all), vec![Edge::Right, Edge::Top, Edge::Bottom]);
        assert_eq!(outer_edges(&laptop, &all), vec![Edge::Left, Edge::Top, Edge::Bottom]);
        // Solo, los cuatro.
        assert_eq!(outer_edges(&main, &[main]).len(), 4);
    }

    #[test]
    fn el_techo_de_una_pantalla_mas_baja_al_lado_es_exterior() {
        let tall = Rect::new(0.0, 0.0, 1536.0, 960.0);
        let low = Rect::new(1536.0, 240.0, 1280.0, 720.0);
        let edges = outer_edges(&low, &[tall, low]);
        assert!(edges.contains(&Edge::Top) && !edges.contains(&Edge::Left));
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

    #[test]
    fn el_notch_de_arriba_no_cambia() {
        // Como antes: 440 de ancho centrado bajo el borde, el alto hacia abajo.
        let rect = notch_rect(Edge::Top, &WORK, 960.0, 440.0, 300.0);
        assert_eq!(rect, Rect::new(740.0, 0.0, 440.0, 300.0));
        assert!(notch_fits(Edge::Top, &Rect::new(0.0, 0.0, 300.0, 200.0), 440.0, 640.0));
    }

    #[test]
    fn el_notch_lateral_crece_desde_su_borde_de_pie() {
        // A la izquierda: pegado al borde, el mismo panel de pie, centrado en
        // la altura del tab.
        let left = notch_rect(Edge::Left, &WORK, 500.0, 440.0, 300.0);
        assert_eq!(left, Rect::new(0.0, 350.0, 440.0, 300.0));
        // A la derecha, espejado: su borde derecho es el de la pantalla.
        let right = notch_rect(Edge::Right, &WORK, 500.0, 440.0, 300.0);
        assert_eq!(right, Rect::new(1480.0, 350.0, 440.0, 300.0));
        assert_eq!(right.right(), WORK.right());
    }

    #[test]
    fn el_notch_lateral_no_se_sale_del_area_de_trabajo() {
        // Con el tab pegado a la esquina de arriba, el panel baja lo justo.
        let near_top = notch_rect(Edge::Left, &WORK, 62.0, 440.0, 460.0);
        assert_eq!(near_top.y, 0.0);
        // Y pegado a la barra de tareas, sube.
        let near_bottom = notch_rect(Edge::Right, &WORK, 1000.0, 440.0, 460.0);
        assert_eq!(near_bottom.bottom(), WORK.bottom());
        // En un monitor muy bajo no cabe: la pill vuela arriba.
        let low = Rect::new(0.0, 0.0, 1366.0, 560.0);
        assert!(!notch_fits(Edge::Left, &low, 440.0, 640.0));
        assert!(notch_fits(Edge::Left, &WORK, 440.0, 640.0));
    }

    #[test]
    fn el_panel_se_mide_de_pie() {
        assert_eq!(Edge::Left.extent(380.0, 900.0), (900.0, 380.0));
        assert_eq!(Edge::Top.extent(440.0, 300.0), (440.0, 300.0));
    }

    #[test]
    fn el_bloque_sale_al_lado_de_la_herramienta() {
        // A la izquierda, a la derecha de la columna y centrado en la herramienta.
        let left = side_block(Edge::Left, &WORK, 40.0, 400.0, 300.0, 120.0);
        assert_eq!(left, Rect::new(40.0, 340.0, 300.0, 120.0));
        // A la derecha, a su izquierda.
        let right = side_block(Edge::Right, &WORK, 40.0, 400.0, 300.0, 120.0);
        assert_eq!((right.x, right.right()), (1580.0, 1880.0));
        // Cerca de una esquina se corre para no salirse.
        assert_eq!(side_block(Edge::Left, &WORK, 40.0, 10.0, 300.0, 120.0).y, 0.0);
        assert_eq!(side_block(Edge::Left, &WORK, 40.0, 1030.0, 300.0, 120.0).bottom(), WORK.bottom());
    }

    /// Punto dentro de un polígono (regla par-impar).
    fn inside(poly: &[(f32, f32)], (x, y): (f32, f32)) -> bool {
        let mut hit = false;
        let mut j = poly.len() - 1;
        for i in 0..poly.len() {
            let ((xi, yi), (xj, yj)) = (poly[i], poly[j]);
            if (yi > y) != (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi {
                hit = !hit;
            }
            j = i;
        }
        hit
    }

    #[test]
    fn la_columna_y_el_bloque_son_una_sola_forma() {
        // Columna de la tira (0..40, 248..752) y el bloque a la altura del
        // portapapeles.
        let column = Rect::new(0.0, 248.0, 40.0, 504.0);
        let block = Rect::new(40.0, 300.0, 300.0, 150.0);
        let poly = side_outline(Edge::Left, &WORK, &column, &block, 22.0, 14.0);
        assert!(inside(&poly, (20.0, 700.0)), "la columna");
        assert!(inside(&poly, (200.0, 380.0)), "el bloque");
        assert!(!inside(&poly, (200.0, 600.0)), "nada de espacio muerto bajo el bloque");
        assert!(!inside(&poly, (200.0, 280.0)), "ni sobre él");
        // La unión es cóncava: un poco de piel en el rincón, no un ángulo vivo.
        assert!(inside(&poly, (42.0, 302.0)) && inside(&poly, (41.0, 298.0)));
        // Las esquinas que miran al escritorio son redondas.
        assert!(!inside(&poly, (339.0, 301.0)));
        // A la derecha, espejado.
        let column_r = Rect::new(1880.0, 248.0, 40.0, 504.0);
        let block_r = Rect::new(1580.0, 300.0, 300.0, 150.0);
        let poly_r = side_outline(Edge::Right, &WORK, &column_r, &block_r, 22.0, 14.0);
        assert!(inside(&poly_r, (1720.0, 380.0)) && inside(&poly_r, (1900.0, 700.0)));
        assert!(!inside(&poly_r, (1720.0, 600.0)));
    }

    #[test]
    fn el_bloque_puede_ser_mas_alto_que_el_tab() {
        // El aviso de la bandeja junto al tab en reposo: sobresale por los
        // dos lados y sigue siendo una forma.
        let column = Rect::new(0.0, 454.0, 40.0, 124.0);
        let block = Rect::new(40.0, 416.0, 300.0, 200.0);
        let poly = side_outline(Edge::Left, &WORK, &column, &block, 22.0, 14.0);
        assert!(inside(&poly, (20.0, 516.0)) && inside(&poly, (200.0, 430.0)));
        assert!(!inside(&poly, (20.0, 420.0)), "sobre la columna, al borde, no hay piel");
        // Bordes casi alineados: sin pasos raros, todo dentro.
        let flush = Rect::new(40.0, 454.3, 300.0, 123.5);
        let poly = side_outline(Edge::Left, &WORK, &column, &flush, 22.0, 14.0);
        assert!(inside(&poly, (100.0, 516.0)) && inside(&poly, (20.0, 460.0)));
    }
}
