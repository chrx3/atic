//! Cambiar de reunión con el «fade through» de M3, como el cambio de conversación de Atic
//! Code (`code/swap.rs`): la que sale se desvanece rápido (el resorte de efectos rápido de
//! las salidas) y la que entra aparece después con un fundido y subiendo unos píxeles con
//! el resorte espacial; los primeros bloques del cuerpo entran escalonados encima de eso.
//!
//! Fade through y no eje compartido: dos reuniones de la lista no tienen relación espacial
//! entre sí. GPUI no escala elementos: la escala de la guía se aproxima con la subida, y
//! tampoco conserva lo que sale: mientras dura la salida, el detalle dibuja una copia quieta
//! de la reunión anterior (`Ghost`) debajo de la nueva, solo esos ~120 ms.
//!
//! El reloj es el del cambio (`Swap::at`, guardado en la vista), no el de los elementos.
//! Todo se asienta en `total()` segundos y deja de pedir cuadros; con movimiento reducido
//! no anima ni dibuja la anterior.

use std::time::Instant;

use gpui_m3::Spring;

/// Cuánto se sigue dibujando la reunión que sale: a los 120 ms el resorte de salida
/// (`EFFECTS_FAST`) ya la dejó casi transparente.
pub(super) const GHOST_SECS: f32 = 0.12;
/// La que entra empieza cuando la otra ya casi se fue.
const IN_DELAY: f32 = 0.06;
/// Cuánto sube lo que entra (en vez de la escala 0,92 → 1).
const RISE: f32 = 8.;
/// Los primeros bloques del cuerpo que entran escalonados, cuánto suben de más y el paso
/// entre ellos.
pub(super) const STAGGER_BLOCKS: usize = 4;
const BLOCK_RISE: f32 = 12.;
const BLOCK_STEP: f32 = 0.04;

/// El cambio de reunión del detalle.
pub(super) struct Swap {
    /// La reunión que se muestra ahora (`None`: ninguna).
    pub key: Option<String>,
    /// Cuándo cambió; `None` sin animación (al abrir la ventana, con movimiento reducido).
    pub at: Option<Instant>,
}

/// Lo que se dibuja de un cambio en este cuadro.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Frame {
    /// Opacidad de la reunión que sale, mientras se dibuja.
    pub ghost: Option<f32>,
    /// Opacidad y desplazamiento hacia abajo (px) de lo que entra.
    pub opacity: f32,
    pub rise: f32,
}

impl Frame {
    pub const REST: Frame = Frame { ghost: None, opacity: 1., rise: 0. };
}

/// Cuándo termina todo, contando el escalonado de los bloques.
pub(super) fn total() -> f32 {
    IN_DELAY + (STAGGER_BLOCKS - 1) as f32 * BLOCK_STEP + Spring::SPATIAL.settle_time().as_secs_f32()
}

/// El cuadro a `t` segundos del cambio. Con `t >= total()`, `Frame::REST`.
pub(super) fn frame(t: f32) -> Frame {
    if t >= total() {
        return Frame::REST;
    }
    let ghost = (t < GHOST_SECS).then(|| (1. - Spring::EFFECTS_FAST.step(t)).clamp(0., 1.));
    let t_in = (t - IN_DELAY).max(0.);
    let opacity = Spring::EFFECTS.step(t_in).clamp(0., 1.);
    let rise = RISE * (1. - Spring::SPATIAL.step(t_in));
    Frame { ghost, opacity, rise }
}

/// Opacidad y subida extra del bloque `order` del cuerpo (`0`: el primero). Los que
/// vienen después del escalonado entran con el último.
pub(super) fn block(t: f32, order: usize) -> (f32, f32) {
    let order = order.min(STAGGER_BLOCKS - 1);
    let t = (t - IN_DELAY - order as f32 * BLOCK_STEP).max(0.);
    if t >= Spring::SPATIAL.settle_time().as_secs_f32() {
        return (1., 0.);
    }
    (Spring::EFFECTS.step(t).clamp(0., 1.), BLOCK_RISE * (1. - Spring::SPATIAL.step(t)))
}

/// Anota que se dibuja la reunión `key`. Devuelve si hay que animar el cambio desde ahora:
/// no la primera vez, ni si antes no había reunión, ni con movimiento reducido.
pub(super) fn note(swap: &mut Option<Swap>, key: Option<&str>, reduced: bool) -> bool {
    let Some(s) = swap.as_mut() else {
        *swap = Some(Swap { key: key.map(str::to_string), at: None });
        return false;
    };
    if s.key.as_deref() == key {
        return false;
    }
    let came_back = s.key.is_none();
    s.key = key.map(str::to_string);
    s.at = (!came_back && !reduced && key.is_some()).then(Instant::now);
    s.at.is_some()
}

/// Los segundos desde el cambio, mientras dura.
pub(super) fn elapsed(swap: &Option<Swap>, now: Instant) -> Option<f32> {
    let at = swap.as_ref()?.at?;
    let t = now.saturating_duration_since(at).as_secs_f32();
    (t < total()).then_some(t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_que_sale_se_va_antes_de_que_entre_la_otra() {
        let start = frame(0.);
        assert_eq!(start.ghost, Some(1.));
        assert_eq!(start.opacity, 0.);
        assert!((start.rise - RISE).abs() < 1e-4);
        let mid = frame(0.09);
        assert!(mid.ghost.unwrap() < 0.05);
        assert!(mid.opacity > 0. && mid.opacity < 0.75);
        assert_eq!(frame(GHOST_SECS).ghost, None);
    }

    #[test]
    fn se_asienta_pronto_y_deja_todo_en_su_lugar() {
        assert_eq!(frame(total()), Frame::REST);
        for order in 0..STAGGER_BLOCKS + 3 {
            assert_eq!(block(total(), order), (1., 0.));
        }
        assert!(total() < 0.7);
    }

    #[test]
    fn los_bloques_entran_de_arriba_hacia_abajo() {
        let t = IN_DELAY + 0.05;
        let (first, _) = block(t, 0);
        let (last, _) = block(t, STAGGER_BLOCKS - 1);
        assert!(first > last);
        // Los de más abajo entran con el último del escalonado.
        assert_eq!(block(t, 9), block(t, STAGGER_BLOCKS - 1));
        assert_eq!(block(0., 2), (0., BLOCK_RISE));
    }

    #[test]
    fn solo_cambiar_de_reunion_anima() {
        let mut swap = None;
        // Al abrir la ventana: nada.
        assert!(!note(&mut swap, Some("a"), false));
        assert!(!note(&mut swap, Some("a"), false));
        assert!(note(&mut swap, Some("b"), false));
        let now = Instant::now();
        assert!(elapsed(&swap, now).is_some());
        // Con movimiento reducido: cambia sin animar.
        assert!(!note(&mut swap, Some("c"), true));
        assert!(elapsed(&swap, now).is_none());
        // Sin reunión (se borró la última) y de vuelta: la que vuelve no anima.
        assert!(!note(&mut swap, None, false));
        assert!(!note(&mut swap, Some("d"), false));
    }
}
