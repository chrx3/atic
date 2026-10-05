//! Movimiento del detalle: el fundido con que entra el contenido al cambiar
//! de reunión o de pestaña, la píldora de las pestañas que se desliza y la
//! navegación de los menús con el teclado.
//!
//! Todo usa `with_animation` con un id que cambia solo cuando cambia lo que
//! se muestra: GPUI arranca la animación con cada id nuevo y, terminada, no
//! pide más cuadros. Volver a dibujar (el resumen que llega, el reproductor
//! que avanza) no la repite.

use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::{
    div, prelude::*, px, Animation, AnimationExt, AnyElement, Bounds, ElementId, Hsla, Pixels,
};

/// Lo que tarda en entrar el contenido del detalle y un menú.
pub(super) const FADE: Duration = Duration::from_millis(140);
/// Lo que se desplaza al entrar: apenas se nota, pero no aparece de golpe.
pub(super) const RISE: f32 = 4.0;
/// La píldora de las pestañas va de una a otra.
const SLIDE: Duration = Duration::from_millis(180);

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

/// La píldora encendida de las pestañas. En reposo la pinta la pestaña
/// elegida (no hace falta medir nada); al cambiar, una píldora aparte va
/// de la posición de una a la otra, con las medidas del cuadro anterior.
#[derive(Default)]
pub(super) struct TabSlide {
    /// Dónde quedaron las dos pestañas en el último cuadro (ventana).
    slots: Rc<Cell<[Option<Bounds<Pixels>>; 2]>>,
    shown: Cell<Option<usize>>,
    /// Cuándo empezó el último cambio, su número y desde qué pestaña.
    switch: Cell<Option<(Instant, u64, usize)>>,
    seq: Cell<u64>,
}

/// Un cambio de pestaña a medio camino.
pub(super) struct Sliding {
    seq: u64,
    from: usize,
    to: usize,
    slots: [Bounds<Pixels>; 2],
}

impl TabSlide {
    /// Anota la pestaña que se dibuja; si cambió desde el cuadro anterior,
    /// empieza a deslizarse.
    pub(super) fn observe(&self, active: usize) -> Option<Sliding> {
        let shown = self.shown.replace(Some(active));
        if shown.is_some_and(|s| s != active) {
            self.seq.set(self.seq.get() + 1);
            self.switch.set(Some((Instant::now(), self.seq.get(), shown.unwrap_or(0))));
        }
        let (at, seq, from) = self.switch.get()?;
        if at.elapsed() >= SLIDE || from == active {
            return None;
        }
        let [Some(a), Some(b)] = self.slots.get() else {
            return None;
        };
        Some(Sliding { seq, from, to: active, slots: [a, b] })
    }

    /// Para `on_children_prepainted` de la fila: las dos pestañas son
    /// siempre los dos últimos hijos (la píldora, si está, va primero para
    /// quedar detrás del texto).
    pub(super) fn recorder(&self) -> impl Fn(Vec<Bounds<Pixels>>, &mut gpui::Window, &mut gpui::App) + 'static {
        let slots = self.slots.clone();
        move |bounds, _, _| {
            if let [.., a, b] = bounds.as_slice() {
                slots.set([Some(*a), Some(*b)]);
            }
        }
    }
}

impl Sliding {
    /// La píldora que viaja, en coordenadas de la fila (`pad`: su relleno).
    pub(super) fn pill(&self, pad: f32, color: Hsla, radius: f32) -> AnyElement {
        let origin = self.slots[0].origin.x;
        let x = |ix: usize| f32::from(self.slots[ix].origin.x - origin) + pad;
        let w = |ix: usize| f32::from(self.slots[ix].size.width);
        let (x0, x1, w0, w1) = (x(self.from), x(self.to), w(self.from), w(self.to));
        let h = f32::from(self.slots[self.to].size.height);
        div()
            .absolute()
            .top(px(pad))
            .h(px(h))
            .rounded(px(radius))
            .bg(color)
            .with_animation(
                ElementId::NamedInteger("tab-pill".into(), self.seq),
                Animation::new(SLIDE).with_easing(gpui::ease_in_out),
                move |el, t| el.left(px(x0 + (x1 - x0) * t)).w(px(w0 + (w1 - w0) * t)),
            )
            .into_any_element()
    }

    /// El color del texto de la pestaña `ix` mientras la píldora pasa:
    /// la que se deja se apaga y la nueva se oscurece sobre la píldora.
    pub(super) fn ink(&self, ix: usize, on: Hsla, off: Hsla, el: impl IntoElement) -> AnyElement {
        let (from, to) = if ix == self.to { (off, on) } else if ix == self.from { (on, off) } else { (off, off) };
        div()
            .text_color(from)
            .child(el)
            .with_animation(
                ElementId::NamedInteger(if ix == 0 { "tab-ink-0" } else { "tab-ink-1" }.into(), self.seq),
                Animation::new(SLIDE).with_easing(gpui::ease_in_out),
                move |el, t| el.text_color(crate::hover::mix(from, to, t)),
            )
            .into_any_element()
    }
}

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
    fn la_pildora_solo_se_mueve_al_cambiar_de_pestana() {
        let slide = TabSlide::default();
        let b = |x: f32| Bounds::new(gpui::point(px(x), px(0.)), gpui::size(px(80.), px(26.)));
        slide.slots.set([Some(b(0.)), Some(b(82.))]);
        assert!(slide.observe(0).is_none(), "la primera vez no hay de dónde venir");
        assert!(slide.observe(0).is_none());
        let moving = slide.observe(1).expect("cambió de pestaña");
        assert_eq!((moving.from, moving.to), (0, 1));
        assert!(slide.observe(1).is_some(), "sigue mientras dura");
    }
}
