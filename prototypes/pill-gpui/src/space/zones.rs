//! Los espacios en la pizarra: cada uno es una zona del plano que envuelve a
//! sus consolas.
//!
//! La zona no se guarda: sale del rectángulo que ocupan las consolas del
//! espacio, con un margen y un encabezado arriba. Si se arrastra una consola,
//! la zona crece con ella. Una consola nueva de un espacio aparece al lado de
//! las que ya tiene; la de un espacio sin consolas, a la derecha de todo.

use super::Area;

/// Aire entre las consolas y el borde de la zona.
pub const PAD: f32 = 28.0;
/// El encabezado de la zona, en unidades del plano.
pub const HEAD: f32 = 46.0;
/// Separación entre consolas al acomodarlas.
const GAP: f32 = 40.0;

pub struct Zone {
    pub id: u64,
    pub area: Area,
}

/// El rectángulo que envuelve a estas áreas.
fn bounds(areas: &[Area]) -> Option<Area> {
    let first = areas.first()?;
    let (mut x0, mut y0, mut x1, mut y1) = (first.x, first.y, first.x + first.w, first.y + first.h);
    for a in &areas[1..] {
        x0 = x0.min(a.x);
        y0 = y0.min(a.y);
        x1 = x1.max(a.x + a.w);
        y1 = y1.max(a.y + a.h);
    }
    Some(Area { x: x0, y: y0, w: x1 - x0, h: y1 - y0 })
}

/// Las zonas de los espacios que tienen consolas, en el orden de `spaces`.
pub fn zones(spaces: &[u64], cards: &[(Option<u64>, Area)]) -> Vec<Zone> {
    spaces
        .iter()
        .filter_map(|&id| {
            let areas: Vec<Area> = cards.iter().filter(|(w, _)| *w == Some(id)).map(|(_, a)| *a).collect();
            let inner = bounds(&areas)?;
            Some(Zone {
                id,
                area: Area {
                    x: inner.x - PAD,
                    y: inner.y - PAD - HEAD,
                    w: inner.w + PAD * 2.0,
                    h: inner.h + PAD * 2.0 + HEAD,
                },
            })
        })
        .collect()
}

/// Dónde va una consola nueva: al lado de las de su espacio,
/// o, si es la primera, a la derecha de todo lo que hay (fuera de toda zona).
/// `None` si el plano está vacío: la decide quien llama.
pub fn place(workspace: Option<u64>, cards: &[(Option<u64>, Area)]) -> Option<(f32, f32)> {
    let mine: Vec<Area> = cards.iter().filter(|(w, _)| workspace.is_some() && *w == workspace).map(|(_, a)| *a).collect();
    if let Some(zone) = bounds(&mine) {
        return Some((zone.x + zone.w + GAP, zone.y));
    }
    let all: Vec<Area> = cards.iter().map(|(_, a)| *a).collect();
    let everything = bounds(&all)?;
    // Fuera de la zona más a la derecha, con su margen y algo más.
    Some((everything.x + everything.w + PAD * 2.0 + GAP * 2.0, everything.y))
}

/// Acomoda áreas en una grilla casi cuadrada que empieza en `origin`, en el
/// orden en que vienen. Cada celda mide lo que la consola más grande.
pub fn arrange(areas: &[Area], origin: (f32, f32)) -> Vec<(f32, f32)> {
    if areas.is_empty() {
        return Vec::new();
    }
    let cols = (areas.len() as f32).sqrt().ceil() as usize;
    let cell_w = areas.iter().map(|a| a.w).fold(0.0, f32::max) + GAP;
    let cell_h = areas.iter().map(|a| a.h).fold(0.0, f32::max) + GAP;
    (0..areas.len())
        .map(|index| (origin.0 + (index % cols) as f32 * cell_w, origin.1 + (index / cols) as f32 * cell_h))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area(x: f32, y: f32) -> Area {
        Area { x, y, w: 100.0, h: 50.0 }
    }

    #[test]
    fn la_zona_envuelve_a_sus_consolas_con_margen_y_encabezado() {
        let cards = [(Some(1), area(0.0, 0.0)), (Some(1), area(200.0, 100.0)), (Some(2), area(900.0, 0.0)), (None, area(-500.0, 0.0))];
        let zones = zones(&[1, 2, 3], &cards);
        assert_eq!(zones.len(), 2);
        assert_eq!(zones[0].area, Area { x: -PAD, y: -PAD - HEAD, w: 300.0 + PAD * 2.0, h: 150.0 + PAD * 2.0 + HEAD });
    }

    #[test]
    fn lo_nuevo_va_al_lado_de_su_espacio_o_a_la_derecha_de_todo() {
        let cards = [(Some(1), area(0.0, 0.0)), (Some(2), area(400.0, 20.0))];
        assert_eq!(place(Some(1), &cards), Some((140.0, 0.0)));
        assert_eq!(place(Some(9), &cards), Some((500.0 + PAD * 2.0 + 80.0, 0.0)));
        assert_eq!(place(Some(1), &[]), None);
    }

    #[test]
    fn ordenar_hace_una_grilla_casi_cuadrada() {
        let areas = [area(0.0, 0.0), area(0.0, 0.0), area(0.0, 0.0)];
        assert_eq!(arrange(&areas, (10.0, 20.0)), vec![(10.0, 20.0), (150.0, 20.0), (10.0, 110.0)]);
    }
}
