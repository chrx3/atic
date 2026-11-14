//! Cambiar de conversación con el «fade through» de M3: la que sale se desvanece rápido
//! (el resorte de efectos rápido de las salidas, ~90 ms) y la que entra aparece después con
//! un fundido (efectos) y subiendo unos píxeles con el resorte espacial; las últimas filas
//! del hilo entran escalonadas encima de eso.
//!
//! Fade through y no eje compartido: dos conversaciones no tienen relación espacial entre
//! sí (no hay «antes» ni «después» en la barra que el movimiento deba contar), que es justo
//! el caso de fade through en la guía de M3. El eje compartido X queda para el Historial ↔
//! chat, que sí son hermanos con dirección (`page-swap` en `view.rs`).
//!
//! GPUI no escala elementos: la escala 0,92 → 1 de la guía se aproxima con la subida.
//! GPUI tampoco conserva lo que sale: mientras dura la salida, el chat dibuja la lista de la
//! conversación anterior (su `ListState`, con lo que ya midió) en una capa debajo de la nueva,
//! solo esos ~120 ms.
//!
//! El reloj es el del cambio (`Swap::at`, guardado en la vista), no el de los elementos: las
//! filas virtuales que se arman a mitad de la entrada la toman en su punto. Todo se asienta
//! en `total()` segundos y deja de pedir cuadros; con movimiento reducido no anima ni dibuja
//! la anterior.

use std::time::Instant;

use gpui_m3::Spring;

/// Cuánto se sigue dibujando la conversación que sale: a los 120 ms el resorte de salida
/// (`EFFECTS_FAST`) ya la dejó en 0,5 % de opacidad.
const GHOST_SECS: f32 = 0.12;
/// La que entra empieza cuando la otra ya casi se fue (le queda ~12 %).
const IN_DELAY: f32 = 0.06;
/// Cuánto sube el hilo que entra (en vez de la escala 0,92 → 1).
const RISE: f32 = 8.;
/// Las últimas filas que entran escalonadas, cuánto suben de más y el paso entre ellas.
pub(super) const STAGGER_ROWS: usize = 4;
const ROW_RISE: f32 = 12.;
const ROW_STEP: f32 = 0.04;

/// El cambio de conversación de un panel.
pub(super) struct Swap {
    pub pane: usize,
    /// La conversación que se muestra ahora (`None`: el Historial o nada).
    pub key: Option<String>,
    /// Lo de antes era un hilo (no el inicio ni el Historial): se puede dibujar saliendo.
    pub thread: bool,
    /// La que sale, mientras se desvanece.
    pub from: Option<String>,
    /// Cuándo cambió; `None` sin animación (al abrir la ventana, al volver del Historial).
    pub at: Option<Instant>,
    /// Cuántas partes tenía al abrirse: las que lleguen después entran solas (`fresh`).
    pub base_len: usize,
}

/// Lo que se dibuja de un cambio en este cuadro.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Frame {
    /// Opacidad de la conversación que sale, mientras se dibuja.
    pub ghost: Option<f32>,
    /// Opacidad y desplazamiento hacia abajo (px) del hilo que entra.
    pub opacity: f32,
    pub rise: f32,
}

impl Frame {
    pub const REST: Frame = Frame { ghost: None, opacity: 1., rise: 0. };
}

/// Cuándo termina todo, contando el escalonado de las últimas filas.
pub(super) fn total() -> f32 {
    IN_DELAY + (STAGGER_ROWS - 1) as f32 * ROW_STEP + Spring::SPATIAL.settle_time().as_secs_f32()
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

/// Opacidad y subida extra de la fila `order` de las últimas (`0`: la de más arriba).
pub(super) fn row(t: f32, order: usize) -> (f32, f32) {
    let t = (t - IN_DELAY - order as f32 * ROW_STEP).max(0.);
    let p = Spring::SPATIAL.step(t);
    if t >= Spring::SPATIAL.settle_time().as_secs_f32() {
        return (1., 0.);
    }
    (Spring::EFFECTS.step(t).clamp(0., 1.), ROW_RISE * (1. - p))
}

/// En qué lugar de las últimas `STAGGER_ROWS` va la fila `ix` de una lista de `rows` filas
/// cuya última es el pie (que entra con el hilo, sin escalonar).
pub(super) fn stagger_order(ix: usize, rows: usize) -> Option<usize> {
    let parts = rows.checked_sub(1)?;
    let n = parts.min(STAGGER_ROWS);
    let first = parts - n;
    (ix >= first && ix < parts).then(|| ix - first)
}

/// Lo que hay que anotar al dibujar el panel `pane` mostrando `key` (`None`: Historial o
/// inicio sin conversación) con `len` partes; `thread` si se ve su hilo. Devuelve si hay
/// que animar el cambio desde ahora.
pub(super) fn note(swap: &mut Option<Swap>, pane: usize, key: Option<&str>, thread: bool, history: bool, len: usize, reduced: bool) -> bool {
    let Some(s) = swap.as_mut().filter(|s| s.pane == pane) else {
        // Primer dibujo del panel (o pasó a ser el activo): sin animar.
        *swap = Some(Swap { pane, key: key.map(str::to_string), thread, from: None, at: None, base_len: len });
        return false;
    };
    if history {
        // El Historial tiene su propia transición: al volver, el hilo entra con ella.
        s.key = None;
        s.thread = false;
        s.from = None;
        s.at = None;
        return false;
    }
    if s.key.as_deref() == key {
        s.thread = thread;
        return false;
    }
    let came_back = s.key.is_none();
    s.from = if s.thread { s.key.take() } else { None };
    s.key = key.map(str::to_string);
    s.thread = thread;
    s.base_len = len;
    s.at = (!came_back && !reduced && key.is_some()).then(Instant::now);
    s.at.is_some()
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
        // A los 90 ms la anterior casi no se ve y la nueva recién empieza.
        let mid = frame(0.09);
        assert!(mid.ghost.unwrap() < 0.05);
        assert!(mid.opacity > 0. && mid.opacity < 0.75);
        // Pasado el tiempo de la salida, ya no se dibuja.
        assert_eq!(frame(GHOST_SECS).ghost, None);
    }

    #[test]
    fn se_asienta_y_deja_todo_en_su_lugar() {
        assert_eq!(frame(total()), Frame::REST);
        assert_eq!(frame(10.), Frame::REST);
        // Las filas escalonadas terminan dentro del total.
        for order in 0..STAGGER_ROWS {
            assert_eq!(row(total(), order), (1., 0.));
        }
        // El total no se alarga de más: menos de 0,7 s.
        assert!(total() < 0.7);
    }

    #[test]
    fn las_filas_entran_de_arriba_hacia_abajo() {
        let t = IN_DELAY + 0.05;
        let (first, _) = row(t, 0);
        let (last, _) = row(t, STAGGER_ROWS - 1);
        assert!(first > last);
        assert_eq!(row(0., 2), (0., ROW_RISE));
    }

    #[test]
    fn solo_las_ultimas_filas_escalonan() {
        // 10 filas: 9 partes y el pie. Escalonan las 4 últimas partes (5..9).
        assert_eq!(stagger_order(4, 10), None);
        assert_eq!(stagger_order(5, 10), Some(0));
        assert_eq!(stagger_order(8, 10), Some(3));
        assert_eq!(stagger_order(9, 10), None);
        // Con menos partes que el escalonado, todas desde la primera.
        assert_eq!(stagger_order(0, 3), Some(0));
        assert_eq!(stagger_order(1, 3), Some(1));
        assert_eq!(stagger_order(0, 0), None);
    }

    #[test]
    fn cambiar_de_conversacion_anima_y_lo_demas_no() {
        let mut swap = None;
        // Al abrir la ventana: nada.
        assert!(!note(&mut swap, 0, Some("a"), true, false, 5, false));
        // La misma: nada.
        assert!(!note(&mut swap, 0, Some("a"), true, false, 6, false));
        // Otra: anima y la anterior sale.
        assert!(note(&mut swap, 0, Some("b"), true, false, 3, false));
        let s = swap.as_ref().unwrap();
        assert_eq!(s.from.as_deref(), Some("a"));
        assert_eq!(s.base_len, 3);
        // Con movimiento reducido: cambia sin animar.
        assert!(!note(&mut swap, 0, Some("c"), true, false, 1, true));
        assert!(swap.as_ref().unwrap().at.is_none());
        // Al Historial y de vuelta a otra: la transición es la del Historial.
        assert!(!note(&mut swap, 0, None, false, true, 0, false));
        assert!(!note(&mut swap, 0, Some("d"), true, false, 2, false));
        // A la pantalla de inicio de otra (la anima su propia entrada) y de ahí a un hilo:
        // entra, pero no hay hilo que salga.
        note(&mut swap, 0, Some("e"), false, false, 0, false);
        assert!(note(&mut swap, 0, Some("f"), true, false, 4, false));
        assert_eq!(swap.as_ref().unwrap().from, None);
        // Otro panel pasa a ser el activo: sin animar.
        assert!(!note(&mut swap, 1, Some("a"), true, false, 5, false));
    }
}
