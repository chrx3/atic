//! Los paneles del Mando: el espacio de la consola se divide hacia el lado o
//! hacia abajo, como las terminales de VS Code o Windows Terminal.
//!
//! Es un árbol: cada hoja es un panel con una consola (o vacío, a la espera de
//! que se elija una) y cada nodo parte su rectángulo en dos con una proporción.
//! Una consola está a lo más en un panel; las que no están en ninguno siguen
//! corriendo y se ven en la barra lateral.

use super::Area;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    /// Uno al lado del otro.
    Row,
    /// Uno sobre el otro.
    Column,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    Left,
    Right,
    Up,
    Down,
}

enum Node {
    Pane(usize),
    Split { axis: Axis, ratio: f32, a: Box<Node>, b: Box<Node> },
}

/// La raya entre dos paneles, para arrastrarla.
pub struct Divider {
    /// Cómo llegar al nodo desde la raíz: `false` = a, `true` = b.
    pub path: Vec<bool>,
    pub axis: Axis,
    /// Donde se agarra.
    pub grip: Area,
    /// Lo que reparte ese nodo, para pasar la posición del mouse a proporción.
    pub span: Area,
}

pub struct Panes {
    root: Node,
    /// Qué consola tiene cada panel.
    slots: Vec<(usize, Option<u64>)>,
    pub focused: usize,
    next: usize,
}

impl Default for Panes {
    fn default() -> Self {
        Self { root: Node::Pane(0), slots: vec![(0, None)], focused: 0, next: 1 }
    }
}

const MIN_RATIO: f32 = 0.15;

impl Panes {
    pub fn count(&self) -> usize {
        self.slots.len()
    }

    pub fn card_in(&self, pane: usize) -> Option<u64> {
        self.slots.iter().find(|(p, _)| *p == pane).and_then(|(_, card)| *card)
    }

    pub fn pane_of(&self, card: u64) -> Option<usize> {
        self.slots.iter().find(|(_, c)| *c == Some(card)).map(|(p, _)| *p)
    }

    pub fn shown(&self) -> Vec<u64> {
        self.slots.iter().filter_map(|(_, card)| *card).collect()
    }

    /// Pone una consola en un panel. Si ya estaba en otro, ese queda vacío.
    pub fn put(&mut self, pane: usize, card: Option<u64>) {
        for slot in &mut self.slots {
            if card.is_some() && slot.1 == card {
                slot.1 = None;
            }
            if slot.0 == pane {
                slot.1 = card;
            }
        }
    }

    /// Dónde va una consola que se pide ver: el panel enfocado si está vacío,
    /// si no otro vacío, y si no hay, el enfocado.
    pub fn place(&self) -> usize {
        if self.card_in(self.focused).is_none() {
            return self.focused;
        }
        self.slots.iter().find(|(_, card)| card.is_none()).map_or(self.focused, |(pane, _)| *pane)
    }

    /// Una consola que se cerró deja su panel vacío.
    pub fn forget(&mut self, alive: &[u64]) {
        for slot in &mut self.slots {
            if slot.1.is_some_and(|card| !alive.contains(&card)) {
                slot.1 = None;
            }
        }
    }

    /// Parte un panel en dos; el nuevo queda vacío y enfocado.
    pub fn split(&mut self, pane: usize, axis: Axis) -> usize {
        let new = self.next;
        self.next += 1;
        if let Some(node) = find(&mut self.root, pane) {
            let old = std::mem::replace(node, Node::Pane(pane));
            *node = Node::Split { axis, ratio: 0.5, a: Box::new(old), b: Box::new(Node::Pane(new)) };
            self.slots.push((new, None));
            self.focused = new;
        }
        new
    }

    /// Quita un panel; su hermano ocupa su lugar. El último no se quita: queda
    /// vacío.
    pub fn close(&mut self, pane: usize) {
        if self.slots.len() <= 1 {
            self.put(pane, None);
            return;
        }
        if remove(&mut self.root, pane) {
            self.slots.retain(|(p, _)| *p != pane);
            if self.focused == pane {
                self.focused = self.slots.first().map_or(0, |(p, _)| *p);
            }
        }
    }

    pub fn layout(&self, area: Area, gap: f32) -> Vec<(usize, Area)> {
        let mut out = Vec::new();
        lay(&self.root, area, gap, &mut out);
        out
    }

    pub fn dividers(&self, area: Area, gap: f32) -> Vec<Divider> {
        let mut out = Vec::new();
        grips(&self.root, area, gap, &mut Vec::new(), &mut out);
        out
    }

    pub fn set_ratio(&mut self, path: &[bool], ratio: f32) {
        let mut node = &mut self.root;
        for &side in path {
            match node {
                Node::Split { a, b, .. } => node = if side { b } else { a },
                Node::Pane(_) => return,
            }
        }
        if let Node::Split { ratio: r, .. } = node {
            *r = ratio.clamp(MIN_RATIO, 1.0 - MIN_RATIO);
        }
    }

    /// El panel vecino en esa dirección: el más cercano que se le enfrenta.
    pub fn neighbor(&self, pane: usize, dir: Dir, area: Area) -> Option<usize> {
        let rects = self.layout(area, 0.0);
        let from = rects.iter().find(|(p, _)| *p == pane)?.1;
        let (cx, cy) = (from.x + from.w / 2.0, from.y + from.h / 2.0);
        rects
            .iter()
            .filter(|(p, _)| *p != pane)
            .filter(|(_, r)| match dir {
                Dir::Left => r.x + r.w <= from.x + 1.0 && overlaps(r.y, r.h, from.y, from.h),
                Dir::Right => r.x >= from.x + from.w - 1.0 && overlaps(r.y, r.h, from.y, from.h),
                Dir::Up => r.y + r.h <= from.y + 1.0 && overlaps(r.x, r.w, from.x, from.w),
                Dir::Down => r.y >= from.y + from.h - 1.0 && overlaps(r.x, r.w, from.x, from.w),
            })
            .min_by(|(_, a), (_, b)| {
                let d = |r: &Area| (r.x + r.w / 2.0 - cx).abs() + (r.y + r.h / 2.0 - cy).abs();
                d(a).total_cmp(&d(b))
            })
            .map(|(p, _)| *p)
    }
}

fn overlaps(a: f32, a_len: f32, b: f32, b_len: f32) -> bool {
    a < b + b_len - 1.0 && b < a + a_len - 1.0
}

fn find(node: &mut Node, pane: usize) -> Option<&mut Node> {
    match node {
        Node::Pane(p) if *p == pane => Some(node),
        Node::Pane(_) => None,
        Node::Split { a, b, .. } => match find(a, pane) {
            Some(found) => Some(found),
            None => find(b, pane),
        },
    }
}

/// Saca la hoja `pane` y sube a su hermano. Dice si la encontró.
fn remove(node: &mut Node, pane: usize) -> bool {
    let Node::Split { a, b, .. } = node else {
        return false;
    };
    let keep = if matches!(**a, Node::Pane(p) if p == pane) {
        Some(std::mem::replace(&mut **b, Node::Pane(usize::MAX)))
    } else if matches!(**b, Node::Pane(p) if p == pane) {
        Some(std::mem::replace(&mut **a, Node::Pane(usize::MAX)))
    } else {
        None
    };
    match keep {
        Some(sibling) => {
            *node = sibling;
            true
        }
        None => remove(a, pane) || remove(b, pane),
    }
}

fn halves(axis: Axis, ratio: f32, area: Area, gap: f32) -> (Area, Area) {
    match axis {
        Axis::Row => {
            let w = ((area.w - gap) * ratio).round();
            (Area { w, ..area }, Area { x: area.x + w + gap, w: area.w - w - gap, ..area })
        }
        Axis::Column => {
            let h = ((area.h - gap) * ratio).round();
            (Area { h, ..area }, Area { y: area.y + h + gap, h: area.h - h - gap, ..area })
        }
    }
}

fn lay(node: &Node, area: Area, gap: f32, out: &mut Vec<(usize, Area)>) {
    match node {
        Node::Pane(pane) => out.push((*pane, area)),
        Node::Split { axis, ratio, a, b } => {
            let (first, second) = halves(*axis, *ratio, area, gap);
            lay(a, first, gap, out);
            lay(b, second, gap, out);
        }
    }
}

fn grips(node: &Node, area: Area, gap: f32, path: &mut Vec<bool>, out: &mut Vec<Divider>) {
    let Node::Split { axis, ratio, a, b } = node else {
        return;
    };
    let (first, second) = halves(*axis, *ratio, area, gap);
    let grip = match axis {
        Axis::Row => Area { x: first.x + first.w, w: gap, ..area },
        Axis::Column => Area { y: first.y + first.h, h: gap, ..area },
    };
    out.push(Divider { path: path.clone(), axis: *axis, grip, span: area });
    path.push(false);
    grips(a, first, gap, path, out);
    path.pop();
    path.push(true);
    grips(b, second, gap, path, out);
    path.pop();
}

#[cfg(test)]
mod tests {
    use super::*;

    const AREA: Area = Area { x: 0.0, y: 0.0, w: 1000.0, h: 600.0 };

    #[test]
    fn dividir_y_cerrar_vuelve_a_un_panel() {
        let mut panes = Panes::default();
        panes.put(0, Some(7));
        let right = panes.split(0, Axis::Row);
        assert_eq!(panes.focused, right);
        let rects = panes.layout(AREA, 10.0);
        assert_eq!(rects.len(), 2);
        assert_eq!(rects[0].1.w, 495.0);
        assert_eq!(rects[1].1.x, 505.0);
        panes.close(right);
        assert_eq!(panes.count(), 1);
        assert_eq!(panes.card_in(0), Some(7));
        assert_eq!(panes.layout(AREA, 10.0)[0].1, AREA);
    }

    #[test]
    fn una_consola_esta_en_un_solo_panel() {
        let mut panes = Panes::default();
        let other = panes.split(0, Axis::Column);
        panes.put(0, Some(3));
        panes.put(other, Some(3));
        assert_eq!(panes.card_in(0), None);
        assert_eq!(panes.pane_of(3), Some(other));
    }

    #[test]
    fn lo_nuevo_va_al_panel_vacio() {
        let mut panes = Panes::default();
        panes.put(0, Some(1));
        let empty = panes.split(0, Axis::Row);
        panes.focused = 0;
        assert_eq!(panes.place(), empty);
        panes.put(empty, Some(2));
        assert_eq!(panes.place(), 0);
    }

    #[test]
    fn los_vecinos_se_buscan_por_posicion() {
        let mut panes = Panes::default();
        let right = panes.split(0, Axis::Row);
        let below = panes.split(right, Axis::Column);
        assert_eq!(panes.neighbor(0, Dir::Right, AREA), Some(right));
        assert_eq!(panes.neighbor(right, Dir::Down, AREA), Some(below));
        assert_eq!(panes.neighbor(below, Dir::Left, AREA), Some(0));
        assert_eq!(panes.neighbor(0, Dir::Up, AREA), None);
    }

    #[test]
    fn la_raya_se_arrastra_dentro_de_un_margen() {
        let mut panes = Panes::default();
        panes.split(0, Axis::Row);
        let divider = &panes.dividers(AREA, 10.0)[0];
        assert!(divider.path.is_empty());
        panes.set_ratio(&[], 0.99);
        assert_eq!(panes.layout(AREA, 0.0)[0].1.w, 850.0);
    }
}
