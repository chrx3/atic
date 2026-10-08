//! Hover con transición.
//!
//! `.hover()` de GPUI cambia el estilo de golpe; en la pill web los fondos se
//! funden (~140 ms). `hover_fx(id, |h| …)` construye el elemento cada cuadro
//! con `h.t` (0 en reposo, 1 con el cursor encima, suavizado) y `h.press`
//! (botón apretado), así cualquier propiedad puede seguir al cursor: el
//! fondo, el color del texto, unas acciones que aparecen, un leve
//! desplazamiento…
//!
//! El estado vive en el elemento (con su `id`): mientras anima pide otro
//! cuadro, y quieto no cuesta nada.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::{
    AnyElement, App, Bounds, DispatchPhase, Element, ElementId, GlobalElementId, Hitbox,
    HitboxBehavior, Hsla, InspectorElementId, IntoElement, LayoutId, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, Pixels, Rgba, Window,
};

/// Entrar es más rápido que salir: responde al toque y se va sin cortar.
pub const ENTER: Duration = Duration::from_millis(120);
pub const LEAVE: Duration = Duration::from_millis(220);
const PRESS_IN: Duration = Duration::from_millis(70);
const PRESS_OUT: Duration = Duration::from_millis(180);

/// Lo que recibe quien dibuja: cuánto hover y cuánto apretado, de 0 a 1.
#[derive(Clone, Copy, Default)]
pub struct Hover {
    pub t: f32,
    pub press: f32,
    /// Solo el cursor, sin contar `lit`: lo que se revela al pasar por
    /// encima (acciones de una fila) no debe salir por elegirla con flechas.
    pub over: f32,
}

impl Hover {
    /// Del color de reposo al de hover.
    pub fn mix(&self, rest: Hsla, hover: Hsla) -> Hsla {
        mix(rest, hover, self.t)
    }
}

/// Mezcla en RGB (en HSL un blanco transparente cambiaría de tono a medio
/// camino).
pub fn mix(a: Hsla, b: Hsla, t: f32) -> Hsla {
    if t <= 0.0 {
        return a;
    }
    if t >= 1.0 {
        return b;
    }
    let (a, b) = (a.to_rgb(), b.to_rgb());
    let l = |x: f32, y: f32| x + (y - x) * t;
    Rgba {
        r: l(a.r, b.r),
        g: l(a.g, b.g),
        b: l(a.b, b.b),
        a: l(a.a, b.a),
    }
    .into()
}

/// Un valor que va hacia 0 o 1 con salida suave y se puede dar vuelta a
/// mitad de camino sin saltos.
#[derive(Clone, Copy)]
struct Track {
    from: f32,
    to: f32,
    start: Instant,
    duration: Duration,
}

impl Track {
    fn new(now: Instant) -> Self {
        Self {
            from: 0.0,
            to: 0.0,
            start: now,
            duration: ENTER,
        }
    }

    fn value(&self, now: Instant) -> f32 {
        let p = (now.saturating_duration_since(self.start).as_secs_f32()
            / self.duration.as_secs_f32().max(0.001))
        .clamp(0.0, 1.0);
        // ease-out cúbica: arranca con el gesto, llega sin golpe.
        let e = 1.0 - (1.0 - p).powi(3);
        self.from + (self.to - self.from) * e
    }

    fn running(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.start) < self.duration
    }

    fn aim(&mut self, on: bool, enter: Duration, leave: Duration, now: Instant) -> bool {
        let to = if on { 1.0 } else { 0.0 };
        if self.to == to {
            return false;
        }
        let from = self.value(now);
        // La duración se acorta en proporción a lo que falta: dar vuelta a
        // medio camino no se siente lento.
        let full = if on { enter } else { leave };
        *self = Self {
            from,
            to,
            start: now,
            duration: full.mul_f32((to - from).abs().max(0.25)),
        };
        true
    }
}

pub struct State {
    hover: Track,
    over: Track,
    press: Track,
    hovered: bool,
    pressed: bool,
}

type Shared = Rc<RefCell<State>>;

pub struct HoverFx {
    id: ElementId,
    build: Option<Box<dyn FnOnce(Hover) -> AnyElement>>,
    enter: Duration,
    leave: Duration,
    lit: bool,
}

/// Envuelve lo que `build` dibuja y le pasa el hover suavizado.
pub fn hover_fx(id: impl Into<ElementId>, build: impl FnOnce(Hover) -> AnyElement + 'static) -> HoverFx {
    HoverFx {
        id: id.into(),
        build: Some(Box::new(build)),
        enter: ENTER,
        leave: LEAVE,
        lit: false,
    }
}

/// Atajos sobre un elemento ya armado: lo que depende del hover se aplica
/// encima de lo demás. El `id` del envoltorio puede ser el mismo del
/// elemento (queda anidado).
pub trait HoverExt: gpui::Styled + IntoElement + Sized + 'static {
    /// Lo general: `f` recibe el elemento y el hover.
    fn fx(self, id: impl Into<ElementId>, f: impl FnOnce(Self, Hover) -> Self + 'static) -> HoverFx {
        hover_fx(id, move |h| f(self, h).into_any_element())
    }

    /// Fondo que se funde de `rest` a `over`; apretado, un poco más.
    fn hover_bg(self, id: impl Into<ElementId>, rest: Hsla, over: Hsla) -> HoverFx {
        hover_fx(id, move |h| {
            let mut pressed = over;
            pressed.a = (over.a + 0.06).min(1.0);
            self.bg(mix(mix(rest, over, h.t), pressed, h.press)).into_any_element()
        })
    }
}

impl<E: gpui::Styled + IntoElement + 'static> HoverExt for E {}

/// El botón redondo de ícono de las franjas (pin, pestañas, «nuevo»):
/// se funde con el cursor y encendido queda con fondo.
pub fn round_button(
    id: &'static str,
    icon: &'static str,
    tip_text: &'static str,
    active: bool,
    text: Hsla,
    faint: Hsla,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
) -> HoverFx {
    use gpui::{div, prelude::*, px, svg};
    hover_fx(id, move |h| {
        let (rest, over) = if active { (0.14, 0.18) } else { (0.0, 0.08) };
        div()
            .id(id)
            .size(px(26.))
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .rounded(px(13.))
            .bg(mix(text.opacity(rest), text.opacity(over + 0.04 * h.press), h.t))
            .cursor_pointer()
            .on_click(on_click)
            .when(!tip_text.is_empty(), |el| el.tooltip(tip(tip_text)))
            .child(
                svg()
                    .path(icon)
                    .size(px(13.))
                    .text_color(if active { text } else { h.mix(faint, text) }),
            )
            .into_any_element()
    })
}

/// El pin de la franja de cada panel.
pub fn pin_button(
    id: &'static str,
    pinned: bool,
    text: Hsla,
    faint: Hsla,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
) -> HoverFx {
    let label = if pinned { "Soltar: se cierra al salir" } else { "Fijar: queda abierto" };
    round_button(id, "icons/pin.svg", label, pinned, text, faint, on_click)
}

/// El globito de ayuda de los botones de ícono (como `use:tip` en la web):
/// GPUI lo muestra tras medio segundo quieto encima; aquí entra fundiéndose.
pub fn tip(text: &'static str) -> impl Fn(&mut Window, &mut App) -> gpui::AnyView + 'static {
    move |_, cx| gpui::AppContext::new(cx, |_| Tip(text.into())).into()
}

/// Como `tip`, con un texto armado en el momento (una ruta, por ejemplo).
pub fn tip_text(text: gpui::SharedString) -> impl Fn(&mut Window, &mut App) -> gpui::AnyView + 'static {
    move |_, cx| gpui::AppContext::new(cx, |_| Tip(text.clone())).into()
}

struct Tip(gpui::SharedString);

impl gpui::Render for Tip {
    fn render(&mut self, _: &mut Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        use gpui::{div, prelude::*, px, rgb, Animation, AnimationExt};
        div()
            .pl(px(10.))
            .pt(px(8.))
            .child(
                div()
                    .px(px(8.))
                    .py(px(4.))
                    .rounded(px(8.))
                    .bg(rgb(0x2a2a27))
                    .font_family("Segoe UI")
                    .text_size(px(11.))
                    .text_color(rgb(0xf0f0ea))
                    .child(self.0.clone())
                    .with_animation(
                        "tip-in",
                        Animation::new(Duration::from_millis(125)).with_easing(gpui::ease_out_quint()),
                        |el, t| el.opacity(t).mt(px(3.0 * (1.0 - t))),
                    ),
            )
    }
}

impl HoverFx {
    /// Encendido aunque el cursor no esté: la fila elegida con el teclado
    /// se realza con la misma transición.
    pub fn lit(mut self, lit: bool) -> Self {
        self.lit = lit;
        self
    }
}

impl IntoElement for HoverFx {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for HoverFx {
    type RequestLayoutState = (AnyElement, Shared);
    type PrepaintState = Hitbox;

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        global_id: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let now = Instant::now();
        let state = window.with_element_state(global_id.expect("hover_fx tiene id"), |state: Option<Shared>, _| {
            let state = state.unwrap_or_else(|| {
                Rc::new(RefCell::new(State {
                    hover: Track::new(now),
                    over: Track::new(now),
                    press: Track::new(now),
                    hovered: false,
                    pressed: false,
                }))
            });
            (state.clone(), state)
        });
        let hover = {
            let s = state.borrow();
            Hover {
                t: s.hover.value(now),
                over: s.over.value(now),
                press: s.press.value(now),
            }
        };
        let build = self.build.take().expect("hover_fx se dibuja una vez");
        let mut element = build(hover);
        let layout = element.request_layout(window, cx);
        (layout, (element, state))
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        (element, _): &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        // Antes que el hijo: su hitbox queda encima y no le quita nada.
        let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
        element.prepaint(window, cx);
        hitbox
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        (element, state): &mut Self::RequestLayoutState,
        hitbox: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        element.paint(window, cx);

        let now = Instant::now();
        let (enter, leave, lit) = (self.enter, self.leave, self.lit);
        let hovered = hitbox.is_hovered(window);
        let running = {
            let mut s = state.borrow_mut();
            s.hovered = hovered;
            if !hovered {
                s.pressed = false;
            }
            s.hover.aim(hovered || lit, enter, leave, now);
            s.over.aim(hovered, enter, leave, now);
            let pressed = s.pressed;
            s.press.aim(pressed, PRESS_IN, PRESS_OUT, now);
            s.hover.running(now) || s.over.running(now) || s.press.running(now)
        };
        if running {
            window.request_animation_frame();
        }

        let view = window.current_view();
        {
            let (hitbox, state) = (hitbox.clone(), state.clone());
            window.on_mouse_event(move |_: &MouseMoveEvent, phase, window, cx| {
                if phase != DispatchPhase::Capture {
                    return;
                }
                let hovered = hitbox.is_hovered(window);
                let mut s = state.borrow_mut();
                if hovered != s.hovered {
                    s.hovered = hovered;
                    if !hovered {
                        s.pressed = false;
                        s.press.aim(false, PRESS_IN, PRESS_OUT, Instant::now());
                    }
                    s.hover.aim(hovered || lit, enter, leave, Instant::now());
                    s.over.aim(hovered, enter, leave, Instant::now());
                    cx.notify(view);
                }
            });
        }
        {
            let (hitbox, state) = (hitbox.clone(), state.clone());
            window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
                if phase == DispatchPhase::Capture
                    && event.button == MouseButton::Left
                    && hitbox.is_hovered(window)
                {
                    let mut s = state.borrow_mut();
                    s.pressed = true;
                    s.press.aim(true, PRESS_IN, PRESS_OUT, Instant::now());
                    cx.notify(view);
                }
            });
        }
        {
            let state = state.clone();
            window.on_mouse_event(move |_: &MouseUpEvent, phase, _, cx| {
                if phase != DispatchPhase::Capture {
                    return;
                }
                let mut s = state.borrow_mut();
                if s.pressed {
                    s.pressed = false;
                    s.press.aim(false, PRESS_IN, PRESS_OUT, Instant::now());
                    cx.notify(view);
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dar_vuelta_a_medio_camino_no_salta() {
        let t0 = Instant::now();
        let mut track = Track::new(t0);
        track.aim(true, ENTER, LEAVE, t0);
        let mid = t0 + ENTER / 2;
        let before = track.value(mid);
        assert!(before > 0.0 && before < 1.0);
        track.aim(false, ENTER, LEAVE, mid);
        assert!((track.value(mid) - before).abs() < 1e-4);
        assert_eq!(track.value(mid + LEAVE), 0.0);
    }

    #[test]
    fn llega_y_se_queda() {
        let t0 = Instant::now();
        let mut track = Track::new(t0);
        assert!(!track.aim(false, ENTER, LEAVE, t0));
        track.aim(true, ENTER, LEAVE, t0);
        assert_eq!(track.value(t0 + ENTER), 1.0);
        assert!(!track.running(t0 + ENTER));
    }

    #[test]
    fn la_mezcla_va_de_un_color_al_otro() {
        let a: Hsla = gpui::white().opacity(0.0);
        let b: Hsla = gpui::white().opacity(0.1);
        assert_eq!(mix(a, b, 0.0), a);
        assert_eq!(mix(a, b, 1.0), b);
        assert!((mix(a, b, 0.5).a - 0.05).abs() < 1e-3);
    }
}
