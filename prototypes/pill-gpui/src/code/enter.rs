//! Entradas de Atic Code en Expressive: un elemento sube (o se desliza) y se funde con el
//! resorte espacial de M3, con un retardo para escalonar listas. Es el `m3-rise`, `m3-row-in`,
//! `m3-part-in` y `m3-panel` de la referencia (`motion.css`); gpui-m3 solo trae las entradas de sus
//! propios componentes, así que lo demás se hace aquí con `motion::entrance` y `motion::replay`.
//!
//! GPUI no escala un elemento, así que se aproxima con desplazamiento y opacidad. El estado de
//! la entrada vive solo mientras el elemento se dibuja: al volver a mostrarse, entra de nuevo.
//! Con movimiento reducido no anima (lo resuelve `motion`).

use gpui::{App, ElementId, Styled, Window, px};
use gpui_m3::Spring;
use gpui_m3::motion::{entrance, replay};

#[derive(Clone)]
pub struct Enter {
    id: ElementId,
    delay: f32,
    dx: f32,
    dy: f32,
    spring: Spring,
    /// Con una generación, entra cada vez que cambia (no al montarse): el cambio de conversación.
    generation: Option<u64>,
}

impl Enter {
    /// `id` debe ser único dentro de la vista; uno nuevo hace entrar de nuevo al elemento.
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self { id: id.into(), delay: 0., dx: 0., dy: 12., spring: Spring::SPATIAL, generation: None }
    }

    /// Segundos de espera antes de empezar.
    pub fn delay(mut self, delay: f32) -> Self {
        self.delay = delay;
        self
    }

    /// De cuántos píxeles viene (`dx` positivo: desde la derecha; `dy` positivo: desde abajo).
    pub fn from(mut self, dx: f32, dy: f32) -> Self {
        self.dx = dx;
        self.dy = dy;
        self
    }

    pub fn spring(mut self, spring: Spring) -> Self {
        self.spring = spring;
        self
    }

    /// Entra cada vez que `generation` cambia, en vez de al aparecer.
    pub fn on_change(mut self, generation: u64) -> Self {
        self.generation = Some(generation);
        self
    }

    /// Avance de la entrada, de 0 a 1 (con el rebote del resorte); exactamente 1 al terminar.
    fn progress(self, window: &mut Window, cx: &mut App) -> f32 {
        match self.generation {
            None => entrance(self.id, self.spring, self.delay, window, cx),
            Some(generation) => {
                let duration = self.spring.settle_time().as_secs_f32() + self.delay;
                match replay(self.id, generation, duration, window, cx) {
                    Some(t) => self.spring.step((t - self.delay).max(0.)),
                    None => 1.,
                }
            }
        }
    }

    /// Aplica la entrada al elemento. Si ya terminó lo devuelve intacto.
    pub fn apply<E: Styled>(self, el: E, window: &mut Window, cx: &mut App) -> E {
        let (dx, dy) = (self.dx, self.dy);
        let p = self.progress(window, cx);
        if p == 1. {
            return el;
        }
        el.relative().left(px((1. - p) * dx)).top(px((1. - p) * dy)).opacity(p.clamp(0., 1.))
    }
}

/// Retardo de la fila `index` de una lista: `step` segundos entre filas, con un tope de
/// `max` filas para que una lista larga no tarde en terminar de entrar.
pub fn stagger(index: usize, step: f32, max: usize) -> f32 {
    index.min(max) as f32 * step
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_escalonado_tiene_tope() {
        assert_eq!(stagger(0, 0.03, 6), 0.);
        assert!((stagger(2, 0.03, 6) - 0.06).abs() < 1e-6);
        assert!((stagger(40, 0.03, 6) - 0.18).abs() < 1e-6);
    }
}
