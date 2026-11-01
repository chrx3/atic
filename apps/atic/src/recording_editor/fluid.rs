//! El movimiento de la interfaz del editor: diálogos, pestañas, controles,
//! bloques de la línea de tiempo y avisos. Skill `transiciones-fluidas` y rúbrica de
//! `docs/EDITOR_PULIDO_2.md`.
//!
//! Reglas que sigue todo lo de aquí:
//! - **El estado manda** (regla 3): cada animación es un valor que va hacia un destino
//!   (abierto/cerrado, elegido/no, una posición) y se puede dar vuelta a mitad de camino
//!   sin saltos (`Anim::aim`, `Anim::flip`). Nada depende de cuántas veces se dibujó.
//! - **Un solo dueño por elemento** (regla 1): lo que cambia de contenido (el panel de una
//!   pestaña, el paso del diálogo) entra con un fundido y la copia anterior se va de golpe,
//!   nunca dos copias fundiéndose.
//! - **Quieto no cuesta nada** (rendimiento): `Fluid` anota si alguna animación corre en
//!   este cuadro y el editor pide el siguiente cuadro solo entonces. Ninguna de estas
//!   animaciones toca el `Composer` ni la cámara: son capas de GPUI encima o al lado.
//! - **Movimiento reducido:** con «Reducir movimiento» del sistema (la misma consulta de
//!   `crate::code::style::system_reduced_motion`, con un caché corto) se conserva el cambio
//!   de estado pero se quitan el desplazamiento y la escala; queda un fundido muy corto
//!   (`REDUCED_FADE`) o nada (lo que es solo posición salta directo).
//!
//! El estado vive en `Fluid`, con un nombre por animación: el editor se dibuja entero en
//! cada cuadro y las piezas (`&self`) lo consultan con `toggle`, `glide` y `since`.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::time::{Duration, Instant};

use gpui::{Hsla, SharedString};

use crate::anim::{ease_smooth_out, lerp};
use crate::hover::{mix, Hover};

// --- Duraciones, distancias y curva: todo en un solo lugar ------------------------------
//
// La curva de todo es `ease_smooth_out` (entra rápido y frena): la respuesta se siente al
// tiro y llega sin golpe. Las salidas son más cortas que las entradas.

/// Diálogo de exportar: el fondo atenuado y la tarjeta (que además sube un poco).
pub(super) const SCRIM_IN: Duration = Duration::from_millis(140);
pub(super) const CARD_IN: Duration = Duration::from_millis(180);
pub(super) const DIALOG_OUT: Duration = Duration::from_millis(110);
/// La tarjeta sube estos px al entrar (GPUI no escala un `div`: el «0,97 → 1» es un ascenso).
pub(super) const CARD_RISE: f32 = 8.0;
/// El cruce entre pasos del diálogo (Ajustes → Exportando → Listo).
pub(super) const STEP_FADE: Duration = Duration::from_millis(140);

/// Pestañas de la barra lateral: la píldora que se desliza y el panel que entra.
pub(super) const TAB_PILL: Duration = Duration::from_millis(160);
pub(super) const TAB_BODY: Duration = Duration::from_millis(140);
/// Cuánto entra de lado el panel nuevo (px), hacia la pestaña a la que se va.
pub(super) const TAB_SHIFT: f32 = 9.0;

/// La tarjeta de lo elegido entra con un fundido (sin animar su alto: empujaría lo de abajo en cada cuadro).
pub(super) const CARD_FADE: Duration = Duration::from_millis(140);

/// Controles: la manilla crece, el valor sigue a un clic, el interruptor se desliza.
pub(super) const THUMB_GROW: Duration = Duration::from_millis(100);
pub(super) const SLIDER_GLIDE: Duration = Duration::from_millis(120);
pub(super) const SWITCH_SLIDE: Duration = Duration::from_millis(120);
/// El fundido de color de una opción (chip, muestra, fila) al elegirla.
pub(super) const CHIP_FADE: Duration = Duration::from_millis(100);
/// Cuánto se acerca al color de «elegido» una opción mientras se aprieta (0–1).
pub(super) const PRESS_TOWARD_SEL: f32 = 0.7;
/// La perilla de un interruptor se alarga estos px mientras se aprieta.
pub(super) const KNOB_PRESS: f32 = 2.0;

/// Línea de tiempo: un bloque nuevo (fundido y escala), la selección, la guía y el rango.
pub(super) const BLOCK_IN: Duration = Duration::from_millis(140);
pub(super) const BLOCK_FROM_SCALE: f32 = 0.92;
pub(super) const BLOCK_SELECT: Duration = Duration::from_millis(100);
pub(super) const GUIDE_FADE: Duration = Duration::from_millis(100);
/// La pista tenue de una pista vacía («Z para agregar un zoom») se va al crear el primer bloque.
pub(super) const HINT_FADE: Duration = Duration::from_millis(100);
/// El resalte de una pista al pasar el mouse: su opacidad de reposo y de hover.
pub(super) const LANE_REST: f32 = 0.6;
pub(super) const LANE_HOVER: f32 = 0.92;

/// Avisos bajo la barra de arriba: entran y salen deslizándose desde arriba.
pub(super) const NOTICE_IN: Duration = Duration::from_millis(160);
pub(super) const NOTICE_OUT: Duration = Duration::from_millis(160);
pub(super) const NOTICE_RISE: f32 = 12.0;

/// Vista previa: la insignia, el recuadro de apuntar y «Preparando…».
pub(super) const OVERLAY_IN: Duration = Duration::from_millis(120);
pub(super) const OVERLAY_OUT: Duration = Duration::from_millis(100);

/// La exportación: la barra sigue al avance que informa el exportador, «cancelando» apaga la barra y la
/// palomita de «Listo» entra con un leve rebote.
pub(super) const PROGRESS_GLIDE: Duration = Duration::from_millis(150);
pub(super) const CANCEL_FADE: Duration = Duration::from_millis(140);
pub(super) const DONE_POP: Duration = Duration::from_millis(220);

/// La vista de la línea de tiempo se desliza con los botones y atajos de zoom (la rueda y el arrastre van 1:1).
pub(super) const VIEW_GLIDE: Duration = Duration::from_millis(180);
/// El eco de un clic sobre la vista previa (el ícono de reproducir o pausar): entra, se queda y sale.
pub(super) const ECHO_IN: Duration = Duration::from_millis(90);
pub(super) const ECHO_HOLD: Duration = Duration::from_millis(500);
pub(super) const ECHO_OUT: Duration = Duration::from_millis(160);
/// El eco entra desde esta parte de su tamaño.
pub(super) const ECHO_FROM: f32 = 0.85;

/// Con «Reducir movimiento»: lo más que dura un fundido.
pub(super) const REDUCED_FADE: Duration = Duration::from_millis(60);
/// Cada cuánto se vuelve a preguntarle al sistema por «Reducir movimiento».
const REDUCED_CACHE: Duration = Duration::from_secs(1);
/// Dar vuelta a medio camino dura esta parte, como mínimo, de la duración completa.
const MIN_SHARE: f32 = 0.25;

// --- Lo puro: progreso, geometría, dirección --------------------------------------------

/// De 0 a 1: cuánto va de `start` a `start + dur`, visto desde `now`.
pub(super) fn progress(start: Instant, now: Instant, dur: Duration) -> f32 {
    if dur.is_zero() {
        return 1.0;
    }
    (now.saturating_duration_since(start).as_secs_f32() / dur.as_secs_f32()).clamp(0.0, 1.0)
}

/// `ease_smooth_out` exacta en los extremos (la curva da 0,9999 en 1).
fn eased(p: f32) -> f32 {
    if p <= 0.0 {
        0.0
    } else if p >= 1.0 {
        1.0
    } else {
        ease_smooth_out(p)
    }
}

/// Cuánto sube (px) lo que entra, dado su progreso `t`; con movimiento reducido, nada.
pub(super) fn lift(t: f32, dist: f32, reduced: bool) -> f32 {
    if reduced {
        0.0
    } else {
        dist * (1.0 - t)
    }
}

/// La escala de lo que crece al entrar, de `from` a 1; con movimiento reducido, siempre 1.
pub(super) fn grow(t: f32, from: f32, reduced: bool) -> f32 {
    if reduced {
        1.0
    } else {
        lerp(from, 1.0, t)
    }
}

/// Hacia dónde se va al pasar de la pestaña `from` a la `to`: 1 a la derecha, −1 a la
/// izquierda, 0 si es la misma.
pub(super) fn slide_dir(from: usize, to: usize) -> f32 {
    match to.cmp(&from) {
        std::cmp::Ordering::Greater => 1.0,
        std::cmp::Ordering::Less => -1.0,
        std::cmp::Ordering::Equal => 0.0,
    }
}

/// Dónde queda la píldora de las pestañas: `pos` es la posición en pestañas (0 = la
/// primera, 1,5 = a medio camino entre la segunda y la tercera). Devuelve la `x` de su
/// borde izquierdo y su ancho, en una fila de `width` px con `pad` por los lados y `gap` entre
/// pestañas iguales.
pub(super) fn pill_rect(pos: f32, count: usize, width: f32, pad: f32, gap: f32) -> (f32, f32) {
    let n = count.max(1) as f32;
    let tab = ((width - pad * 2.0 - gap * (n - 1.0)) / n).max(0.0);
    (pad + pos.clamp(0.0, n - 1.0) * (tab + gap), tab)
}

/// Cuánto de «elegida» tiene la pestaña `index` con la píldora en `pos` (1 debajo de ella, 0 a
/// una pestaña o más de distancia): el ícono se enciende mientras la píldora llega.
pub(super) fn tab_weight(pos: f32, index: usize) -> f32 {
    (1.0 - (pos - index as f32).abs()).clamp(0.0, 1.0)
}

/// El fondo de una opción de un grupo exclusivo: del reposo (transparente) al hover y, si
/// está elegida (`on`, 0–1), al color de elegida. Apretarla la empuja hacia ese color.
pub(super) fn select_bg(h: Hover, on: f32, hover: Hsla, sel: Hsla) -> Hsla {
    let idle = mix(hover.opacity(0.0), hover, h.t);
    mix(idle, sel, on.max(PRESS_TOWARD_SEL * h.press))
}

// --- Un valor que va hacia un destino ---------------------------------------------------

/// Un valor que va de `from` a `to` en `dur` con `ease_smooth_out`, y se puede redirigir a
/// mitad de camino partiendo del valor de ese instante.
#[derive(Clone, Copy, Debug)]
pub(super) struct Anim {
    from: f32,
    to: f32,
    start: Instant,
    dur: Duration,
}

impl Anim {
    /// Quieto en `value`.
    pub(super) fn at(value: f32, now: Instant) -> Self {
        Self { from: value, to: value, start: now, dur: Duration::ZERO }
    }

    pub(super) fn value(&self, now: Instant) -> f32 {
        let p = progress(self.start, now, self.dur);
        if p >= 1.0 {
            self.to
        } else {
            lerp(self.from, self.to, eased(p))
        }
    }

    pub(super) fn target(&self) -> f32 {
        self.to
    }

    pub(super) fn running(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.start) < self.dur
    }

    /// Hacia `to` en `dur`, desde donde esté. El mismo destino no la reinicia.
    pub(super) fn aim(&mut self, to: f32, dur: Duration, now: Instant) {
        if self.to == to {
            return;
        }
        *self = Self { from: self.value(now), to, start: now, dur };
    }

    /// Para un 0 ↔ 1 (abierto/cerrado, elegido/no): `enter` al ir a 1 y `leave` al ir a 0. Dar
    /// vuelta a medio camino dura en proporción a lo que falta (con un mínimo): no se siente lenta.
    pub(super) fn flip(&mut self, on: bool, enter: Duration, leave: Duration, now: Instant) {
        let to = if on { 1.0 } else { 0.0 };
        if self.to == to {
            return;
        }
        let from = self.value(now);
        let full = if on { enter } else { leave };
        *self = Self { from, to, start: now, dur: full.mul_f32((to - from).abs().max(MIN_SHARE)) };
    }

    /// Salta a `value` sin animar.
    pub(super) fn snap(&mut self, value: f32, now: Instant) {
        *self = Self::at(value, now);
    }
}

// --- El registro de animaciones del editor ----------------------------------------------

/// Todas las animaciones nombradas del editor y si alguna corre en este cuadro.
#[derive(Default)]
pub(super) struct Fluid {
    anims: RefCell<HashMap<SharedString, Anim>>,
    running: Cell<bool>,
    reduced: Cell<Option<(Instant, bool)>>,
}

impl Fluid {
    pub(super) fn new() -> Self {
        Self::default()
    }

    /// Al empezar a dibujar un cuadro: nada corre todavía.
    pub(super) fn begin_frame(&self) {
        self.running.set(false);
    }

    /// Al terminar de armar el cuadro: ¿hay que pedir otro? (alguna animación sigue en curso)
    pub(super) fn animating(&self) -> bool {
        self.running.get()
    }

    /// «Reducir movimiento» del sistema, preguntado como mucho una vez por segundo.
    pub(super) fn reduced(&self) -> bool {
        let now = Instant::now();
        if let Some((at, value)) = self.reduced.get() {
            if now.saturating_duration_since(at) < REDUCED_CACHE {
                return value;
            }
        }
        let value = crate::code::style::system_reduced_motion();
        self.reduced.set(Some((now, value)));
        value
    }

    /// Un fundido corto en vez del largo si hay que reducir el movimiento.
    fn fade(&self, dur: Duration) -> Duration {
        if self.reduced() {
            dur.min(REDUCED_FADE)
        } else {
            dur
        }
    }

    /// Cuánto sube lo que entra (px).
    pub(super) fn lift(&self, t: f32, dist: f32) -> f32 {
        lift(t, dist, self.reduced())
    }

    /// La escala de lo que crece al entrar.
    pub(super) fn grow(&self, t: f32, from: f32) -> f32 {
        grow(t, from, self.reduced())
    }

    fn drive(&self, key: SharedString, initial: f32, step: impl FnOnce(&mut Anim, Instant)) -> f32 {
        let now = Instant::now();
        let mut anims = self.anims.borrow_mut();
        let anim = anims.entry(key).or_insert_with(|| Anim::at(initial, now));
        step(anim, now);
        if anim.running(now) {
            self.running.set(true);
        }
        anim.value(now)
    }

    /// Un 0 ↔ 1 con nombre: va hacia 1 si `on`, hacia 0 si no. La primera vez nace ya en su
    /// destino (algo que está a medio camino al abrirse no reproduce una entrada que ya pasó).
    pub(super) fn toggle(&self, key: impl Into<SharedString>, on: bool, enter: Duration, leave: Duration) -> f32 {
        let (enter, leave) = (self.fade(enter), self.fade(leave));
        self.drive(key.into(), if on { 1.0 } else { 0.0 }, move |anim, now| anim.flip(on, enter, leave, now))
    }

    /// Un valor cualquiera con nombre que se desliza hacia `target` en `dur`; con `snap`, o con
    /// movimiento reducido, salta directo (para seguir el arrastre sin retraso).
    pub(super) fn glide(&self, key: impl Into<SharedString>, target: f32, dur: Duration, snap: bool) -> f32 {
        let snap = snap || self.reduced();
        self.drive(key.into(), target, move |anim, now| {
            if snap {
                if anim.target() != target || anim.running(now) {
                    anim.snap(target, now);
                }
            } else {
                anim.aim(target, dur, now);
            }
        })
    }

    /// El progreso (0–1, con la curva) de algo que empezó en `at` y dura `dur`: la entrada de lo
    /// que acaba de aparecer o de cambiar. Sale del estado (`at`), no de cuántas veces se dibujó.
    pub(super) fn since(&self, at: Instant, dur: Duration) -> f32 {
        self.since_with(at, dur, eased)
    }

    /// Como `since`, con otra curva (el rebote de la palomita): exacta en 0 y en 1.
    pub(super) fn since_with(&self, at: Instant, dur: Duration, curve: fn(f32) -> f32) -> f32 {
        let dur = self.fade(dur);
        let p = progress(at, Instant::now(), dur);
        if p < 1.0 {
            self.running.set(true);
        }
        if p <= 0.0 {
            0.0
        } else if p >= 1.0 {
            1.0
        } else {
            curve(p)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FULL: Duration = Duration::from_millis(100);

    fn near(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    #[test]
    fn una_animacion_abierta_llega_y_se_queda() {
        let t0 = Instant::now();
        let mut anim = Anim::at(0.0, t0);
        anim.flip(true, FULL, FULL, t0);
        assert!(near(anim.value(t0), 0.0));
        let mid = anim.value(t0 + FULL / 2);
        assert!(mid > 0.0 && mid < 1.0);
        assert_eq!(anim.value(t0 + FULL), 1.0);
        assert!(!anim.running(t0 + FULL));
        assert_eq!(anim.value(t0 + FULL * 5), 1.0);
    }

    #[test]
    fn darle_vuelta_a_medias_parte_del_valor_de_ese_instante() {
        let t0 = Instant::now();
        let mut anim = Anim::at(0.0, t0);
        anim.flip(true, FULL, FULL, t0);
        let mid_at = t0 + FULL / 2;
        let before = anim.value(mid_at);
        anim.flip(false, FULL, FULL, mid_at);
        // Sin salto en el instante de darla vuelta…
        assert!(near(anim.value(mid_at), before));
        // …baja de forma monótona…
        let a = anim.value(mid_at + FULL / 10);
        let b = anim.value(mid_at + FULL / 5);
        assert!(a < before && b < a);
        // …y llega a 0 sin pasarse.
        assert_eq!(anim.value(mid_at + FULL), 0.0);
        assert!(anim.running(mid_at + FULL / 10));
    }

    #[test]
    fn dar_vuelta_cuando_casi_no_se_movio_es_rapido_pero_no_instantaneo() {
        let t0 = Instant::now();
        let mut anim = Anim::at(0.0, t0);
        anim.flip(true, FULL, FULL, t0);
        let soon = t0 + Duration::from_millis(1);
        assert!(anim.value(soon) < 0.25, "casi no se movió");
        anim.flip(false, FULL, FULL, soon);
        // Falta muy poco: dura la cuarta parte (el mínimo), no los 100 ms completos.
        assert!(anim.running(soon + Duration::from_millis(20)));
        assert!(!anim.running(soon + Duration::from_millis(26)));
        assert_eq!(anim.value(soon + Duration::from_millis(26)), 0.0);
    }

    #[test]
    fn abrir_y_cerrar_rapido_repetidamente_no_se_pasa_de_0_a_1() {
        let t0 = Instant::now();
        let mut anim = Anim::at(0.0, t0);
        let mut now = t0;
        for i in 0..40 {
            anim.flip(i % 2 == 0, FULL, FULL, now);
            let v = anim.value(now);
            assert!((0.0..=1.0).contains(&v), "valor fuera de rango: {v}");
            now += Duration::from_millis(7);
        }
        // Al final, un destino claro: se asienta ahí.
        anim.flip(true, FULL, FULL, now);
        assert_eq!(anim.value(now + FULL * 2), 1.0);
    }

    #[test]
    fn el_mismo_destino_no_reinicia() {
        let t0 = Instant::now();
        let mut anim = Anim::at(0.0, t0);
        anim.aim(10.0, FULL, t0);
        let later = t0 + FULL / 3;
        let v = anim.value(later);
        let reference = anim;
        anim.aim(10.0, FULL, later);
        assert!(near(anim.value(later), v));
        // Sigue por la misma curva que si no se hubiera pedido otra vez.
        assert!(near(anim.value(later + FULL / 3), reference.value(later + FULL / 3)));
        assert!(anim.value(later + FULL / 3) > v);
    }

    #[test]
    fn el_snap_salta_sin_animar() {
        let t0 = Instant::now();
        let mut anim = Anim::at(0.0, t0);
        anim.aim(1.0, FULL, t0);
        anim.snap(0.4, t0 + FULL / 4);
        assert_eq!(anim.value(t0 + FULL / 4), 0.4);
        assert!(!anim.running(t0 + FULL / 4 + Duration::from_millis(1)));
    }

    #[test]
    fn el_progreso_sale_del_inicio_y_no_se_pasa() {
        let t0 = Instant::now();
        assert_eq!(progress(t0, t0, FULL), 0.0);
        assert!(near(progress(t0, t0 + FULL / 2, FULL), 0.5));
        assert_eq!(progress(t0, t0 + FULL * 3, FULL), 1.0);
        // Un reloj que va hacia atrás no da negativos.
        assert_eq!(progress(t0 + FULL, t0, FULL), 0.0);
        assert_eq!(progress(t0, t0, Duration::ZERO), 1.0);
    }

    #[test]
    fn la_curva_es_exacta_en_los_extremos_y_sube() {
        assert_eq!(eased(0.0), 0.0);
        assert_eq!(eased(1.0), 1.0);
        assert!(eased(0.25) > 0.25, "sale rápido");
        assert!(eased(0.25) < eased(0.5) && eased(0.5) < eased(0.75));
    }

    #[test]
    fn el_movimiento_reducido_quita_el_desplazamiento_y_la_escala() {
        assert_eq!(lift(0.0, CARD_RISE, true), 0.0);
        assert_eq!(lift(0.0, CARD_RISE, false), CARD_RISE);
        assert_eq!(lift(1.0, CARD_RISE, false), 0.0);
        assert_eq!(grow(0.0, BLOCK_FROM_SCALE, true), 1.0);
        assert!(near(grow(0.0, BLOCK_FROM_SCALE, false), BLOCK_FROM_SCALE));
        assert_eq!(grow(1.0, BLOCK_FROM_SCALE, false), 1.0);
    }

    #[test]
    fn el_panel_entra_desde_el_lado_de_la_pestana_a_la_que_se_va() {
        assert_eq!(slide_dir(0, 3), 1.0, "a la derecha");
        assert_eq!(slide_dir(5, 2), -1.0, "a la izquierda");
        assert_eq!(slide_dir(4, 4), 0.0, "la misma: sin desplazamiento");
        // El desplazamiento inicial es TAB_SHIFT en la dirección del cambio y baja a 0.
        assert_eq!(slide_dir(0, 1) * lift(0.0, TAB_SHIFT, false), TAB_SHIFT);
        assert_eq!(slide_dir(1, 0) * lift(0.0, TAB_SHIFT, false), -TAB_SHIFT);
        assert_eq!(slide_dir(0, 1) * lift(1.0, TAB_SHIFT, false), 0.0);
        assert_eq!(slide_dir(0, 1) * lift(0.0, TAB_SHIFT, true), 0.0);
    }

    #[test]
    fn la_pildora_cae_justo_bajo_cada_pestana() {
        // 7 pestañas en 262 px con 3 de aire y 2 entre ellas: cada una mide (262 − 6 − 12) / 7.
        let (width, pad, gap) = (262.0, 3.0, 2.0);
        let tab = (width - pad * 2.0 - gap * 6.0) / 7.0;
        for i in 0..7 {
            let (left, w) = pill_rect(i as f32, 7, width, pad, gap);
            assert!(near(w, tab));
            assert!(near(left, pad + i as f32 * (tab + gap)));
        }
        // La última termina donde termina el aire derecho.
        let (left, w) = pill_rect(6.0, 7, width, pad, gap);
        assert!(near(left + w, width - pad));
    }

    #[test]
    fn la_pildora_se_desliza_entre_dos_pestanas_sin_cambiar_de_ancho() {
        let (a, wa) = pill_rect(2.0, 7, 262.0, 3.0, 2.0);
        let (b, wb) = pill_rect(3.0, 7, 262.0, 3.0, 2.0);
        let (m, wm) = pill_rect(2.5, 7, 262.0, 3.0, 2.0);
        assert!(near(wa, wb) && near(wa, wm));
        assert!(near(m, (a + b) / 2.0));
        // Fuera de rango queda en la punta (la píldora nunca se sale de la fila).
        assert!(near(pill_rect(-3.0, 7, 262.0, 3.0, 2.0).0, 3.0));
        assert!(near(pill_rect(99.0, 7, 262.0, 3.0, 2.0).0, pill_rect(6.0, 7, 262.0, 3.0, 2.0).0));
        // Una fila sin medir no da anchos negativos.
        assert_eq!(pill_rect(1.0, 7, 0.0, 3.0, 2.0).1, 0.0);
    }

    #[test]
    fn el_icono_se_enciende_mientras_la_pildora_llega() {
        assert_eq!(tab_weight(2.0, 2), 1.0);
        assert!(near(tab_weight(2.5, 2), 0.5) && near(tab_weight(2.5, 3), 0.5));
        assert_eq!(tab_weight(2.0, 4), 0.0);
        assert_eq!(tab_weight(2.0, 0), 0.0);
    }

    #[test]
    fn el_fondo_de_una_opcion_va_del_reposo_al_elegido() {
        let hover = gpui::white().opacity(0.1);
        let sel = gpui::white().opacity(0.3);
        let rest = Hover::default();
        assert!((select_bg(rest, 0.0, hover, sel).a - 0.0).abs() < 1e-3);
        assert!((select_bg(rest, 1.0, hover, sel).a - 0.3).abs() < 1e-3);
        let half = select_bg(rest, 0.5, hover, sel).a;
        assert!(half > 0.0 && half < 0.3);
        // Apretada, se acerca al color de elegida aunque no lo esté.
        let pressed = select_bg(Hover { t: 1.0, press: 1.0, over: 1.0 }, 0.0, hover, sel).a;
        assert!(pressed > 0.1 && pressed < 0.3);
    }

    #[test]
    fn las_duraciones_son_cortas_y_las_salidas_no_pasan_a_las_entradas() {
        assert!(DIALOG_OUT <= SCRIM_IN && DIALOG_OUT <= CARD_IN);
        for d in [SCRIM_IN, CARD_IN, STEP_FADE, TAB_PILL, TAB_BODY, THUMB_GROW, SLIDER_GLIDE, SWITCH_SLIDE, CHIP_FADE, BLOCK_IN, BLOCK_SELECT, GUIDE_FADE, HINT_FADE, CARD_FADE, VIEW_GLIDE, ECHO_IN, ECHO_OUT, NOTICE_IN, NOTICE_OUT, PROGRESS_GLIDE, CANCEL_FADE, DONE_POP] {
            assert!(d >= Duration::from_millis(90) && d <= DONE_POP, "{d:?}");
        }
        assert!(REDUCED_FADE < CHIP_FADE);
    }
}
