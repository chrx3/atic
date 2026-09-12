//! Enumeración de monitores vía Core Graphics (`CGDisplayBounds`).
//!
//! El espacio global de Quartz está en **puntos**, con el origen en la esquina
//! superior izquierda de la pantalla principal y Y hacia abajo: el mismo
//! convenio que el escritorio virtual de Windows, pero sin multiplicar por la
//! escala de cada monitor. Mezclar escalas por monitor rompía la geometría
//! global (un monitor Retina pasaba a medir el doble que uno 1x, cuando en el
//! espacio en puntos que usa AppKit son vecinos). Los píxeles nativos se
//! consultan solo al capturar, con `scale` (1920/1920 = 1, 2880/1440 = 2).

use core_graphics::display::CGDisplay;

use super::MonitorInfo;
use crate::geometry::Rect;

pub(super) fn enumerate() -> Vec<MonitorInfo> {
    let Ok(ids) = CGDisplay::active_displays() else {
        return Vec::new();
    };
    let main_id = CGDisplay::main().id;
    ids.into_iter()
        .enumerate()
        .map(|(i, id)| {
            let display = CGDisplay::new(id);
            let bounds = rect_from_points(display.bounds());
            let is_primary = id == main_id;
            // `work` = `bounds` arriba a propósito: la barra de menú de macOS
            // no es como la de tareas de Windows. El overlay se pone *por
            // encima* del menú (NSStatusWindowLevel) y la pill se funde con
            // el canto real, igual que el notch de Windows en el techo.
            // El Dock, si hay, se recorta en el overlay con visibleFrame más
            // adelante; recortar el menú aquí apagaba el dintel SDF.
            MonitorInfo {
                id: format!("monitor-{i}"),
                bounds,
                work_area: bounds,
                is_primary,
                scale: native_scale(display),
            }
        })
        .collect()
}

/// Píxeles del backing store por punto del monitor: 1.0 en un display 1x,
/// 2.0 en Retina. Es la escala con la que `CGDisplayCreateImage` entrega la
/// imagen de ese display, no la disposición global.
pub(crate) fn native_scale(display: CGDisplay) -> f64 {
    let width_pt = display.bounds().size.width;
    let pixel_w = display
        .display_mode()
        .map(|m| m.pixel_width())
        .filter(|&w| w > 0)
        .unwrap_or_else(|| display.pixels_wide());
    let scale = if width_pt > 0.5 {
        pixel_w as f64 / width_pt
    } else {
        1.0
    };
    if scale.is_finite() && scale > 0.1 {
        scale
    } else {
        1.0
    }
}

fn rect_from_points(bounds: core_graphics::geometry::CGRect) -> Rect {
    Rect::new(
        bounds.origin.x.round() as i32,
        bounds.origin.y.round() as i32,
        bounds.size.width.round().max(0.0) as u32,
        bounds.size.height.round().max(0.0) as u32,
    )
}

#[cfg(test)]
mod tests {
    use core_graphics::geometry::{CGPoint, CGRect, CGSize};

    use super::rect_from_points;
    use crate::geometry::Rect;

    #[test]
    fn rect_en_puntos_conserva_origen_negativo() {
        let rect = rect_from_points(CGRect {
            origin: CGPoint {
                x: -277.0,
                y: -1080.0,
            },
            size: CGSize {
                width: 1920.0,
                height: 1080.0,
            },
        });
        assert_eq!(rect, Rect::new(-277, -1080, 1920, 1080));
    }
}
