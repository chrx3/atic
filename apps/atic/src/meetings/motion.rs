//! Movimiento del detalle: el fundido con que entra el contenido al cambiar
//! de reunión, el deslizamiento al cambiar de pestaña y la navegación de los
//! menús con el teclado. La píldora de las pestañas es la de `crate::glide`.
//!
//! Todo usa `with_animation` con un id que cambia solo cuando cambia lo que
//! se muestra: GPUI arranca la animación con cada id nuevo y, terminada, no
//! pide más cuadros. Volver a dibujar (el resumen que llega, el reproductor
//! que avanza) no la repite.

use std::cell::Cell;
use std::time::Duration;

use gpui::{div, prelude::*, px, Animation, AnimationExt, AnyElement, ElementId};

/// Lo que tarda en entrar el contenido del detalle y un menú.
pub(super) const FADE: Duration = Duration::from_millis(140);
/// Lo que se desplaza al entrar: apenas se nota, pero no aparece de golpe.
pub(super) const RISE: f32 = 4.0;

/// Envuelve `el` para que entre fundiéndose (y subiendo `rise` px) la
/// primera vez que se dibuja con este `id`.
pub(super) fn fade_in(id: impl Into<ElementId>, rise: f32, el: impl IntoElement) -> AnyElement {
    div()
        .flex_none()
        .relative()
        .child(el)
        .with_animation(
            id,
            Animation::new(FADE).with_easing(gpui::ease_out_quint()),
            move |el, t| el.opacity(t).top(px(rise * (1.0 - t))),
        )
        .into_any_element()
}

/// El cursor de un menú abierto con el teclado: baja y sube dando la
/// vuelta; sin cursor, ↓ va al primero y ↑ al último.
pub(super) fn step_cursor(cursor: Option<usize>, len: usize, delta: isize) -> Option<usize> {
    if len == 0 {
        return None;
    }
    let last = len as isize - 1;
    let next = match cursor {
        None if delta >= 0 => 0,
        None => last,
        Some(ix) => (ix.min(len - 1) as isize + delta).rem_euclid(len as isize),
    };
    Some(next as usize)
}

/// Hacia dónde entra el cuerpo del detalle: al pasar de Resumen a
/// Transcripción viene desde la derecha y al volver, desde la izquierda; al
/// cambiar de reunión solo se funde. Guarda lo último que se mostró.
#[derive(Default)]
pub(super) struct BodySlide {
    shown: std::cell::RefCell<Option<(String, usize)>>,
    dx: Cell<f32>,
}

impl BodySlide {
    /// Anota lo que se dibuja (`rec`, pestaña `active`) y devuelve desde
    /// cuántos px a la derecha entra (negativo: desde la izquierda).
    pub(super) fn observe(&self, rec: &str, active: usize) -> f32 {
        let mut shown = self.shown.borrow_mut();
        let changed = shown.as_ref().map(|(r, a)| (r.as_str() != rec, *a != active));
        match changed {
            Some((false, true)) => self.dx.set(if active > 0 { SIDE } else { -SIDE }),
            Some((true, _)) => self.dx.set(0.),
            _ => {}
        }
        *shown = Some((rec.to_string(), active));
        if crate::code::style::system_reduced_motion() {
            return 0.;
        }
        self.dx.get()
    }
}

/// Cuánto se desliza el cuerpo al cambiar de pestaña.
const SIDE: f32 = 16.0;

/// Como `fade_in`, entrando además `dx` px de lado.
pub(super) fn slide_in(id: impl Into<ElementId>, dx: f32, rise: f32, el: impl IntoElement) -> AnyElement {
    // Llena el alto del scroll: así un estado vacío se centra en el panel
    // en vez de quedar pegado arriba.
    div()
        .flex()
        .flex_col()
        .flex_grow()
        .relative()
        .child(el)
        .with_animation(
            id,
            Animation::new(SLIDE_IN).with_easing(gpui::ease_out_quint()),
            move |el, t| el.opacity(t).left(px(dx * (1.0 - t))).top(px(rise * (1.0 - t))),
        )
        .into_any_element()
}

/// Lo que tarda el cuerpo en entrar al cambiar de pestaña: un poco más que el
/// fundido, para que el deslizamiento se alcance a ver.
const SLIDE_IN: Duration = Duration::from_millis(260);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_cursor_del_menu_da_la_vuelta() {
        assert_eq!(step_cursor(None, 3, 1), Some(0));
        assert_eq!(step_cursor(None, 3, -1), Some(2));
        assert_eq!(step_cursor(Some(0), 3, 1), Some(1));
        assert_eq!(step_cursor(Some(2), 3, 1), Some(0));
        assert_eq!(step_cursor(Some(0), 3, -1), Some(2));
        assert_eq!(step_cursor(Some(1), 0, 1), None);
    }

    #[test]
    fn un_menu_que_se_achico_no_deja_el_cursor_afuera() {
        // «Eliminar» desaparece mientras el menú está abierto: el cursor
        // sigue desde el último que existe.
        assert_eq!(step_cursor(Some(4), 3, 1), Some(0));
        assert_eq!(step_cursor(Some(4), 3, -1), Some(1));
    }

    #[test]
    fn el_cuerpo_entra_desde_el_lado_de_la_pestana() {
        if crate::code::style::system_reduced_motion() {
            return;
        }
        let slide = BodySlide::default();
        assert_eq!(slide.observe("a", 0), 0., "la primera vez solo se funde");
        assert!(slide.observe("a", 1) > 0., "a Transcripción, desde la derecha");
        assert!(slide.observe("a", 1) > 0., "sigue igual mientras no cambie nada");
        assert!(slide.observe("a", 0) < 0., "de vuelta a Resumen, desde la izquierda");
        assert_eq!(slide.observe("b", 0), 0., "otra reunión: solo se funde");
    }
}
