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

/// Los resortes de la capa de estado en M3 Expressive (entrar, salir): de
/// efectos, sin rebote (un resorte espacial sacaría la opacidad de rango).
/// La presión va con el rápido también al entrar: un clic corto tiene que
/// alcanzar a cruzar la mitad (`m3_pressed`) para que el radio se cuadre.
const HOVER_SPRINGS: (gpui_m3::Spring, gpui_m3::Spring) = (gpui_m3::Spring::EFFECTS, gpui_m3::Spring::EFFECTS);
const PRESS_SPRINGS: (gpui_m3::Spring, gpui_m3::Spring) = (gpui_m3::Spring::EFFECTS_FAST, gpui_m3::Spring::EFFECTS_FAST);

/// Cómo se mueve la capa de estado: en Atic, las curvas de arriba; en M3
/// Expressive, los resortes de efectos, como la tira; con movimiento
/// reducido, salta al valor.
#[derive(Clone, Copy, PartialEq)]
enum Pace {
    Curve,
    Spring,
    Jump,
}

impl Pace {
    fn current() -> Self {
        if crate::pill_look::content_scheme().is_none() {
            Pace::Curve
        } else if crate::anim::reduced_motion() {
            Pace::Jump
        } else {
            Pace::Spring
        }
    }
}

/// Un valor que va hacia 0 o 1 con salida suave y se puede dar vuelta a
/// mitad de camino sin saltos.
#[derive(Clone, Copy)]
struct Track {
    from: f32,
    to: f32,
    start: Instant,
    duration: Duration,
    /// Con resorte (Expressive), en vez de la curva de `duration`.
    spring: Option<gpui_m3::Spring>,
    /// La velocidad con que partió el resorte.
    velocity: f32,
}

impl Track {
    fn new(now: Instant) -> Self {
        Self {
            from: 0.0,
            to: 0.0,
            start: now,
            duration: ENTER,
            spring: None,
            velocity: 0.0,
        }
    }

    /// Lo que le falta al resorte para llegar y su velocidad; `None` si ya
    /// llegó o si va por curva.
    fn spring_offset(&self, now: Instant) -> Option<(f32, f32)> {
        let spring = self.spring?;
        let t = now.saturating_duration_since(self.start).as_secs_f32();
        let (x, v) = spring.solve(self.from - self.to, self.velocity, t);
        (x.abs() >= 0.002 || v.abs() >= 0.05).then_some((x, v))
    }

    fn value(&self, now: Instant) -> f32 {
        if self.spring.is_some() {
            return self.spring_offset(now).map_or(self.to, |(x, _)| (self.to + x).clamp(0.0, 1.0));
        }
        let p = (now.saturating_duration_since(self.start).as_secs_f32()
            / self.duration.as_secs_f32().max(0.001))
        .clamp(0.0, 1.0);
        // ease-out cúbica: arranca con el gesto, llega sin golpe.
        let e = 1.0 - (1.0 - p).powi(3);
        self.from + (self.to - self.from) * e
    }

    fn running(&self, now: Instant) -> bool {
        if self.spring.is_some() {
            return self.spring_offset(now).is_some();
        }
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
            spring: None,
            velocity: 0.0,
        };
        true
    }

    /// Como `aim`, con un resorte: dar vuelta a mitad de camino conserva la
    /// velocidad.
    fn aim_spring(&mut self, on: bool, spring: gpui_m3::Spring, now: Instant) -> bool {
        let to = if on { 1.0 } else { 0.0 };
        if self.to == to {
            return false;
        }
        let (from, velocity) = match self.spring_offset(now) {
            Some((x, v)) => (self.to + x, v),
            None => (self.value(now), 0.0),
        };
        *self = Self { from, to, start: now, duration: self.duration, spring: Some(spring), velocity };
        true
    }

    /// Directo al valor, sin animar (movimiento reducido).
    fn jump(&mut self, on: bool, now: Instant) -> bool {
        let to = if on { 1.0 } else { 0.0 };
        if self.to == to && !self.running(now) {
            return false;
        }
        *self = Self { from: to, to, start: now, duration: Duration::ZERO, spring: None, velocity: 0.0 };
        true
    }

    /// Hacia `on` al ritmo de `pace`: la curva (`enter`, `leave`) o el
    /// resorte (`springs`: entrar, salir).
    fn steer(
        &mut self,
        on: bool,
        pace: Pace,
        (enter, leave): (Duration, Duration),
        springs: (gpui_m3::Spring, gpui_m3::Spring),
        now: Instant,
    ) -> bool {
        match pace {
            Pace::Curve => self.aim(on, enter, leave, now),
            Pace::Spring => self.aim_spring(on, if on { springs.0 } else { springs.1 }, now),
            Pace::Jump => self.jump(on, now),
        }
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
    build: Option<Box<dyn FnOnce(Hover, &mut Window, &mut App) -> AnyElement>>,
    enter: Duration,
    leave: Duration,
    lit: bool,
}

/// Envuelve lo que `build` dibuja y le pasa el hover suavizado.
pub fn hover_fx(id: impl Into<ElementId>, build: impl FnOnce(Hover) -> AnyElement + 'static) -> HoverFx {
    HoverFx {
        id: id.into(),
        build: Some(Box::new(move |h, _: &mut Window, _: &mut App| build(h))),
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

    /// Como `fx`, con la ventana y la app: para los resortes de gpui-m3
    /// (`m3_press_radius`).
    fn fx_cx(
        self,
        id: impl Into<ElementId>,
        f: impl FnOnce(Self, Hover, &mut Window, &mut App) -> Self + 'static,
    ) -> HoverFx {
        hover_fx_cx(id, move |h, window, cx| f(self, h, window, cx).into_any_element())
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
    let label = if pinned { crate::i18n::t("pill.clipboard.unpin") } else { crate::i18n::t("pill.clipboard.pin") };
    if !pinned {
        return round_button(id, "icons/pin.svg", label, false, text, faint, on_click);
    }
    // Fijado se nota de lejos: claro sobre la piel oscura, como los botones
    // de Atic Code.
    use gpui::{div, prelude::*, px, svg};
    let ink = crate::pill_look::ink(0x1a1a18);
    hover_fx(id, move |h| {
        div()
            .id(id)
            .size(px(26.))
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .rounded(px(13.))
            .bg(text.opacity(0.86 + 0.14 * h.t))
            .cursor_pointer()
            .on_click(on_click)
            .tooltip(tip(label))
            .child(svg().path("icons/pin.svg").size(px(13.)).text_color(ink))
            .into_any_element()
    })
}

/// `round_button` en M3 Expressive, para los paneles de la pill: un botón de
/// ícono que se alterna, con capa de estado; elegido va en el contenedor
/// secundario. `strong` (el pin fijado) va lleno, en el acento.
pub fn round_button_m3(
    id: &'static str,
    icon: &'static str,
    tip_text: &'static str,
    active: bool,
    strong: bool,
    s: gpui_m3::Scheme,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
) -> HoverFx {
    use gpui::{div, prelude::*, px, svg};
    hover_fx(id, move |h| {
        let layer = 0.08 * h.t + 0.02 * h.press;
        let (bg, fg) = match (active, strong) {
            (true, true) => (gpui_m3::theme::mix(s.on_primary, s.primary, layer), s.on_primary),
            (true, false) => (
                gpui_m3::theme::mix(s.on_secondary_container, s.secondary_container, layer),
                s.on_secondary_container,
            ),
            _ => (s.on_surface_variant.opacity(layer), s.on_surface_variant),
        };
        div()
            .id(id)
            .size(px(26.))
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            // Elegido, un poco más cuadrado (como los botones de alternar de M3).
            .rounded(if active { gpui_m3::corner::S } else { gpui_m3::corner::FULL })
            .bg(bg)
            .cursor_pointer()
            .on_click(on_click)
            .when(!tip_text.is_empty(), |el| el.tooltip(tip(tip_text)))
            .child(svg().path(icon).size(px(13.)).text_color(fg))
            .into_any_element()
    })
}

/// `pin_button` en M3 Expressive: fijado va lleno, en el acento.
pub fn pin_button_m3(
    id: &'static str,
    pinned: bool,
    s: gpui_m3::Scheme,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
) -> HoverFx {
    let label = if pinned { crate::i18n::t("pill.clipboard.unpin") } else { crate::i18n::t("pill.clipboard.pin") };
    round_button_m3(id, "icons/pin.svg", label, pinned, true, s, on_click)
}

/// El globito de ayuda de los botones de ícono (como `use:tip` en la web):
/// GPUI lo muestra tras medio segundo quieto encima; aquí entra fundiéndose.
pub fn tip(text: &'static str) -> impl Fn(&mut Window, &mut App) -> gpui::AnyView + 'static {
    move |_, cx| gpui::AppContext::new(cx, |_| Tip(text.into(), None)).into()
}

/// Como `tip`, con la tecla del atajo como un cuadrito aparte al final («Cortar» y `S`).
pub fn tip_keyed(text: &'static str, key: Option<&'static str>) -> impl Fn(&mut Window, &mut App) -> gpui::AnyView + 'static {
    move |_, cx| gpui::AppContext::new(cx, |_| Tip(text.into(), key.map(Into::into))).into()
}

/// Como `tip`, con un texto armado en el momento (una ruta, por ejemplo).
pub fn tip_text(text: gpui::SharedString) -> impl Fn(&mut Window, &mut App) -> gpui::AnyView + 'static {
    move |_, cx| gpui::AppContext::new(cx, |_| Tip(text.clone(), None)).into()
}

/// El texto del globito y, si lo hay, la tecla del atajo.
struct Tip(gpui::SharedString, Option<gpui::SharedString>);

impl Tip {
    /// El texto y, a su lado, la tecla en un cuadrito (de `color`, con poco relleno).
    /// `m3`: la tecla en LABEL_SMALL (11 px, el mínimo de la escala).
    fn label(&self, color: Hsla, m3: bool) -> gpui::Div {
        use gpui::{div, prelude::*, px};
        use gpui_m3::theme::type_scale::{TypeScaleExt, LABEL_SMALL};
        div().flex().items_center().gap(px(6.)).child(self.0.clone()).children(self.1.clone().map(|key| {
            div()
                .px(px(4.))
                .rounded(px(4.))
                .bg(color.opacity(0.18))
                .map(|el| if m3 { el.type_style(LABEL_SMALL) } else { el.text_size(px(10.)) })
                .child(key)
        }))
    }
}

impl gpui::Render for Tip {
    fn render(&mut self, window: &mut Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        use gpui::{div, prelude::*, px, rgb, Animation, AnimationExt};
        use gpui_m3::theme::type_scale::{TypeScaleExt, BODY_SMALL};
        // M3 Expressive: el globito plano de M3, con los colores dados vuelta
        // (claro sobre la ventana oscura y al revés).
        if let Some(t) = crate::theme::expressive_in(window) {
            return div()
                .pl(px(10.))
                .pt(px(8.))
                .child(
                    div()
                        .min_h(px(24.))
                        .px(px(8.))
                        .py(px(4.))
                        .rounded(gpui_m3::theme::corner::XS)
                        .bg(t.text)
                        .font_family(t.font)
                        .type_style(BODY_SMALL)
                        .text_color(t.pane)
                        .child(self.label(t.pane, true))
                        .with_animation(
                            "tip-in-m3",
                            Animation::new(Duration::from_millis(150)).with_easing(gpui::ease_out_quint()),
                            |el, t| el.opacity(t).mt(px(4.0 * (1.0 - t))),
                        ),
                )
                .into_any_element();
        }
        div()
            .pl(px(10.))
            .pt(px(8.))
            .child(
                div()
                    .px(px(8.))
                    .py(px(4.))
                    .rounded(px(8.))
                    .bg(rgb(0x2a2a27))
                    .font_family(crate::fonts::UI)
                    .text_size(px(11.))
                    .text_color(rgb(0xf0f0ea))
                    .child(self.label(rgb(0xf0f0ea).into(), false))
                    .with_animation(
                        "tip-in",
                        Animation::new(Duration::from_millis(125)).with_easing(gpui::ease_out_quint()),
                        |el, t| el.opacity(t).mt(px(3.0 * (1.0 - t))),
                    ),
            )
            .into_any_element()
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
        let mut element = build(hover, window, cx);
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
        let pace = Pace::current();
        let hovered = hitbox.is_hovered(window);
        let running = {
            let mut s = state.borrow_mut();
            s.hovered = hovered;
            if !hovered {
                s.pressed = false;
            }
            s.hover.steer(hovered || lit, pace, (enter, leave), HOVER_SPRINGS, now);
            s.over.steer(hovered, pace, (enter, leave), HOVER_SPRINGS, now);
            let pressed = s.pressed;
            s.press.steer(pressed, pace, (PRESS_IN, PRESS_OUT), PRESS_SPRINGS, now);
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
                        s.press.steer(false, pace, (PRESS_IN, PRESS_OUT), PRESS_SPRINGS, Instant::now());
                    }
                    s.hover.steer(hovered || lit, pace, (enter, leave), HOVER_SPRINGS, Instant::now());
                    s.over.steer(hovered, pace, (enter, leave), HOVER_SPRINGS, Instant::now());
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
                    s.press.steer(true, pace, (PRESS_IN, PRESS_OUT), PRESS_SPRINGS, Instant::now());
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
                    s.press.steer(false, pace, (PRESS_IN, PRESS_OUT), PRESS_SPRINGS, Instant::now());
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
    fn con_resorte_dar_vuelta_conserva_el_valor_y_se_detiene() {
        let t0 = Instant::now();
        let mut track = Track::new(t0);
        assert!(track.aim_spring(true, gpui_m3::Spring::EFFECTS, t0));
        let mid = t0 + Duration::from_millis(40);
        let before = track.value(mid);
        assert!(before > 0.0 && before < 1.0);
        assert!(track.running(mid));
        track.aim_spring(false, gpui_m3::Spring::EFFECTS_FAST, mid);
        assert!((track.value(mid) - before).abs() < 1e-4);
        let end = mid + gpui_m3::Spring::EFFECTS_FAST.settle_time() * 3;
        assert_eq!(track.value(end), 0.0);
        assert!(!track.running(end));
    }

    #[test]
    fn con_movimiento_reducido_salta() {
        let t0 = Instant::now();
        let mut track = Track::new(t0);
        assert!(track.jump(true, t0));
        assert_eq!(track.value(t0), 1.0);
        assert!(!track.running(t0));
        assert!(!track.jump(true, t0));
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

// --- M3 Expressive: el interior de los paneles ------------------------------
//
// Piezas para los paneles del notch (Portapapeles, Textos, Control, Agentes…)
// en M3 Expressive, cuando `pill_look::content_scheme()` es `Some`. Tienen el
// aspecto de los componentes de gpui-m3 (SearchBar, FilterChip, ListItem,
// estado vacío) pero el clic lo pone cada panel, así no rompen el arrastre ni
// el foco de la pill.

/// Como `hover_fx`, pero `build` recibe también la ventana y la app: para
/// animar con los resortes de gpui-m3 lo que depende del hover
/// (`m3_press_radius`, `m3_press_radii`).
pub fn hover_fx_cx(
    id: impl Into<ElementId>,
    build: impl FnOnce(Hover, &mut Window, &mut App) -> AnyElement + 'static,
) -> HoverFx {
    HoverFx {
        id: id.into(),
        build: Some(Box::new(build)),
        enter: ENTER,
        leave: LEAVE,
        lit: false,
    }
}

/// La capa de estado de M3 sobre `base`: `on` al 8 % con el cursor encima y
/// al 10 % apretado.
pub fn m3_layer(base: Hsla, on: Hsla, h: Hover) -> Hsla {
    gpui_m3::theme::mix(on, base, crate::pill_look::PillTokens::layer_alpha(h.t, h.press))
}

/// ¿Apretado? `h.press` es un fundido corto: a la mitad ya cuenta.
fn m3_pressed(h: Hover) -> bool {
    h.press > 0.5
}

/// El radio que se cuadra al presionar (M3 Expressive): `rest` en reposo,
/// `pressed` apretado, con `SPATIAL_FAST`. Solo dentro de `hover_fx_cx` o
/// `fx_cx`: el resorte vive en el estado de ese elemento. Con movimiento
/// reducido salta al valor.
pub fn m3_press_radius(h: Hover, rest: f32, pressed: f32, window: &mut Window, cx: &mut App) -> f32 {
    let target = if m3_pressed(h) { pressed } else { rest };
    gpui_m3::motion::animate_f32("m3-press-radius", gpui_m3::Spring::SPATIAL_FAST, target, window, cx).max(0.0)
}

/// Las esquinas de arriba y de abajo de una fila de lista agrupada (las
/// listas segmentadas de Expressive): las de afuera del grupo `outer`, las de
/// adentro `inner`; apretada, todas `pressed`. Con `SPATIAL_FAST`, como
/// `m3_press_radius`.
pub fn m3_press_radii(
    h: Hover,
    first: bool,
    last: bool,
    (outer, inner, pressed): (f32, f32, f32),
    window: &mut Window,
    cx: &mut App,
) -> (f32, f32) {
    let target = if m3_pressed(h) {
        [pressed, pressed]
    } else {
        [if first { outer } else { inner }, if last { outer } else { inner }]
    };
    let [top, bottom] = gpui_m3::motion::animate("m3-press-radii", gpui_m3::Spring::SPATIAL_FAST, target, window, cx);
    (top.max(0.0), bottom.max(0.0))
}

/// Las esquinas de una lista agrupada: grandes afuera del grupo, chicas entre
/// filas, y casi cuadradas al apretar (L, XS, XS de la escala de M3).
pub const M3_GROUP_RADII: (f32, f32, f32) = (16.0, 4.0, 4.0);

/// El buscador como SearchBar de M3: una píldora `surface_container_high` con
/// la lupa y el campo adentro.
pub fn m3_search_bar(s: &gpui_m3::Scheme, input: impl IntoElement) -> gpui::Div {
    use gpui::{div, prelude::*, px, svg};
    div()
        .h(px(32.))
        .flex_1()
        .min_w_0()
        .flex()
        .items_center()
        .gap(px(8.))
        .px(px(12.))
        .rounded_full()
        .bg(s.surface_container_high)
        .child(svg().path("icons/search.svg").size(px(14.)).flex_none().text_color(s.on_surface_variant))
        .child(div().flex_1().min_w_0().text_size(px(12.)).line_height(px(16.)).child(input))
}

/// El alto de una fila de chips de filtro (`m3_filter_chip`), con su margen.
pub const M3_CHIPS_H: f32 = 36.0;

/// Un chip de filtro de M3: elegido va en `secondary_container` con su ✓;
/// si no, con contorno. Al presionar se cuadra un poco (`SPATIAL_FAST`).
pub fn m3_filter_chip(
    id: impl Into<ElementId>,
    icon: &'static str,
    label: &'static str,
    selected: bool,
    s: gpui_m3::Scheme,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
) -> HoverFx {
    use gpui::{div, prelude::*, px, svg};
    let id = id.into();
    let inner = id.clone();
    hover_fx_cx(id, move |h, window, cx| {
        let radius = m3_press_radius(h, 8.0, 4.0, window, cx);
        let (base, fg) = if selected {
            (s.secondary_container, s.on_secondary_container)
        } else {
            (s.on_surface_variant.opacity(0.0), s.on_surface_variant)
        };
        div()
            .id(inner)
            .h(px(28.))
            .flex()
            .flex_none()
            .items_center()
            .gap(px(6.))
            .pl(px(8.))
            .pr(px(12.))
            .rounded(px(radius))
            .when(!selected, |el| el.border_1().border_color(s.outline_variant))
            .bg(m3_layer(base, fg, h))
            .cursor_pointer()
            .on_click(on_click)
            .child(
                svg()
                    .path(if selected { "icons/check.svg" } else { icon })
                    .size(px(14.))
                    .flex_none()
                    .text_color(fg),
            )
            .child(
                div()
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(if selected { fg } else { s.on_surface })
                    .child(label),
            )
            .into_any_element()
    })
}

/// El alto del estado vacío de M3 (`m3_empty_state`) con un texto de dos
/// renglones.
pub const M3_EMPTY_H: f32 = 140.0;

/// El estado vacío de M3: una forma grande (de la biblioteca de Expressive)
/// con un ícono adentro y el texto debajo. La forma no se mueve: no pide
/// cuadros. Quien llama puede agregar una acción con `.child`.
pub fn m3_empty_state(
    s: &gpui_m3::Scheme,
    shape: gpui_m3::ShapeName,
    icon: &'static str,
    text: impl Into<gpui::SharedString>,
) -> gpui::Div {
    use gpui::{div, prelude::*, px, svg};
    div()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(10.))
        .px(px(28.))
        .child(
            gpui_m3::Shape::new(shape)
                .size(px(64.))
                .color(s.secondary_container)
                .child(svg().path(crate::icons::themed(icon)).size(px(24.)).text_color(s.on_secondary_container)),
        )
        .child(
            div()
                .max_w(px(320.))
                .text_center()
                .text_size(px(12.))
                .line_height(px(17.))
                .text_color(s.on_surface_variant)
                .child(Into::<gpui::SharedString>::into(text)),
        )
}

/// El contenedor de la cabeza de una fila de lista (ListItem «leading»): un
/// ícono o una miniatura dentro de una forma tonal.
pub fn m3_leading(s: &gpui_m3::Scheme, size: f32, radius: f32, child: impl IntoElement) -> gpui::Div {
    use gpui::{div, prelude::*, px};
    div()
        .size(px(size))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(radius))
        .bg(s.surface_container_highest)
        .overflow_hidden()
        .child(child)
}

/// La entrada escalonada corta del contenido de un panel (M3, `EFFECTS`):
/// 0 → 1 desde que el elemento `id` apareció, `index` partes después de la
/// primera. Con movimiento reducido, 1. Para las partes grandes del panel
/// (la franja, el cuerpo, el pie), no para cada fila de una lista.
pub fn m3_enter(id: impl Into<ElementId>, index: usize, window: &mut Window, cx: &mut App) -> f32 {
    gpui_m3::motion::entrance(id, gpui_m3::Spring::EFFECTS, index as f32 * 0.035, window, cx).clamp(0.0, 1.0)
}

/// Un segmento de un grupo de botones conectado de M3 (ButtonGroup
/// `connected`, SegmentedButton): esquinas chicas entre segmentos y redondas
/// en los extremos del grupo; el elegido se vuelve píldora en
/// `secondary_container` y apretado se cuadra, con `SPATIAL_FAST`. Quien llama
/// los pone en fila con 2 px entre ellos. `grow` reparte el ancho del grupo.
#[allow(clippy::too_many_arguments)]
pub fn m3_segment(
    id: impl Into<ElementId>,
    (index, count): (usize, usize),
    selected: bool,
    icon: Option<&'static str>,
    label: Option<&'static str>,
    tip_text: &'static str,
    (height, grow): (f32, bool),
    s: gpui_m3::Scheme,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
) -> HoverFx {
    use gpui::{div, prelude::*, px, svg};
    let id = id.into();
    let inner = id.clone();
    hover_fx_cx(id, move |h, window, cx| {
        let (round, small, pressed) = (height / 2.0, 4.0, 4.0);
        let target = if m3_pressed(h) {
            [pressed, pressed]
        } else {
            [
                if index == 0 || selected { round } else { small },
                if index + 1 == count || selected { round } else { small },
            ]
        };
        let [left, right] =
            gpui_m3::motion::animate("m3-segment-radii", gpui_m3::Spring::SPATIAL_FAST, target, window, cx);
        let (base, fg) = if selected {
            (s.secondary_container, s.on_secondary_container)
        } else {
            (s.surface_container_high, s.on_surface_variant)
        };
        div()
            .id(inner)
            .h(px(height))
            .when(grow, |el| el.flex_1().min_w_0())
            .when(!grow && label.is_none(), |el| el.w(px(height + 8.0)))
            .when(label.is_some(), |el| el.px(px(12.)))
            .when(!grow, |el| el.flex_none())
            .flex()
            .items_center()
            .justify_center()
            .gap(px(6.))
            .rounded_tl(px(left.max(0.0)))
            .rounded_bl(px(left.max(0.0)))
            .rounded_tr(px(right.max(0.0)))
            .rounded_br(px(right.max(0.0)))
            .bg(m3_layer(base, fg, h))
            .text_color(fg)
            .cursor_pointer()
            .on_click(on_click)
            .when(!tip_text.is_empty(), |el| el.tooltip(tip(tip_text)))
            .children(icon.map(|icon| {
                svg()
                    .path(if selected && label.is_some() { "icons/check.svg" } else { icon })
                    .size(px(14.))
                    .flex_none()
                    .text_color(fg)
            }))
            .children(label.map(|label| {
                div()
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .truncate()
                    .child(label)
            }))
            .into_any_element()
    })
}

// --- M3 Expressive: las barras de las herramientas de pantalla completa -----
//
// Pizarra, Capturas, Color y Flip, cuando `pill_look::content_scheme()` es
// `Some`: barras flotantes (Floating Toolbar de M3 Expressive) con el esquema
// de la pill, para que combinen con lo que el usuario eligió. Como las piezas
// de arriba, solo dan el aspecto: el clic, el arrastre y los atajos los pone
// cada herramienta en su propio elemento, y el estilo va en
// `.fx_cx(id, |el, h, window, cx| m3_tool(el, h, ..))`.

/// El lado de un botón de ícono de las barras de M3.
pub const M3_TOOL: f32 = 40.0;
/// El relleno de una barra alrededor de sus botones.
pub const M3_TOOLBAR_PAD: f32 = 4.0;

/// Cómo se pinta un botón de una barra de M3.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum M3Tone {
    /// Botón de ícono estándar: sin contenedor, solo la capa de estado.
    Standard,
    /// Botón de alternar elegido: en `secondary_container` y más cuadrado.
    Selected,
    /// Tonal, en `secondary_container`, sin cambiar de forma (su menú está abierto).
    Tonal,
    /// Lleno, en `primary`: la acción principal («Listo»).
    Filled,
    /// Lleno, en `error`: grabar.
    Error,
}

/// El contenedor y el contenido de un botón de `tone`.
pub fn m3_tone_colors(tone: M3Tone, s: &gpui_m3::Scheme) -> (Hsla, Hsla) {
    match tone {
        M3Tone::Standard => (s.on_surface_variant.opacity(0.0), s.on_surface_variant),
        M3Tone::Selected | M3Tone::Tonal => (s.secondary_container, s.on_secondary_container),
        M3Tone::Filled => (s.primary, s.on_primary),
        M3Tone::Error => (s.error, s.on_error),
    }
}

/// El radio de la escala de formas de M3 (0, 4, 8, 12, 16, 20, 28, 32, 48)
/// más cercano a `radius`; en un empate, el más chico.
pub fn m3_corner_near(radius: f32) -> f32 {
    const SCALE: [f32; 9] = [0.0, 4.0, 8.0, 12.0, 16.0, 20.0, 28.0, 32.0, 48.0];
    SCALE.into_iter().min_by(|a, b| (a - radius).abs().total_cmp(&(b - radius).abs())).unwrap_or(radius)
}

/// El radio de un botón de barra de alto `height`: redondo en reposo; elegido,
/// más cuadrado (el botón de alternar de Expressive cambia de forma); apretado,
/// más todavía. Con `SPATIAL_FAST`, solo dentro de `hover_fx_cx`/`fx_cx`.
pub fn m3_tool_radius(h: Hover, tone: M3Tone, height: f32, window: &mut Window, cx: &mut App) -> f32 {
    let target = if m3_pressed(h) {
        m3_corner_near(height * 0.2)
    } else if tone == M3Tone::Selected {
        m3_corner_near(height * 0.3)
    } else {
        height / 2.0
    };
    gpui_m3::motion::animate_f32("m3-tool-radius", gpui_m3::Spring::SPATIAL_FAST, target, window, cx).max(0.0)
}

/// El aspecto de un botón de barra: su contenedor con la capa de estado y su
/// radio. Devuelve el elemento y el color de su contenido.
pub fn m3_tool<E: gpui::Styled>(
    el: E,
    h: Hover,
    tone: M3Tone,
    height: f32,
    s: &gpui_m3::Scheme,
    window: &mut Window,
    cx: &mut App,
) -> (E, Hsla) {
    let (base, fg) = m3_tone_colors(tone, s);
    let radius = m3_tool_radius(h, tone, height, window, cx);
    (el.rounded(gpui::px(radius)).bg(m3_layer(base, fg, h)).text_color(fg), fg)
}

/// Un botón de un grupo conectado (ButtonGroup `connected` de Expressive:
/// deshacer, rehacer y borrar; Captura | Grabar): esquinas chicas entre
/// botones y redondas en los extremos; el elegido se vuelve píldora en
/// `selected` (normalmente `secondary_container`) y apretado se cuadra, con
/// `SPATIAL_FAST`. Sin elegir va en `surface_container_highest`, que se ve
/// sobre la barra. Quien llama los pone en fila con 2 px entre ellos.
#[allow(clippy::too_many_arguments)]
pub fn m3_connected<E: gpui::Styled>(
    el: E,
    h: Hover,
    (index, count): (usize, usize),
    selected: Option<(Hsla, Hsla)>,
    height: f32,
    s: &gpui_m3::Scheme,
    window: &mut Window,
    cx: &mut App,
) -> (E, Hsla) {
    use gpui::px;
    let (round, small) = (height / 2.0, m3_corner_near(height * 0.2));
    let pressed = m3_corner_near(small * 0.5).max(4.0);
    let target = if m3_pressed(h) {
        [pressed, pressed]
    } else {
        let chosen = selected.is_some();
        [
            if index == 0 || chosen { round } else { small },
            if index + 1 == count || chosen { round } else { small },
        ]
    };
    let [left, right] =
        gpui_m3::motion::animate("m3-connected-radii", gpui_m3::Spring::SPATIAL_FAST, target, window, cx);
    let (left, right) = (left.max(0.0), right.max(0.0));
    let (base, fg) = selected.unwrap_or((s.surface_container_highest, s.on_surface_variant));
    (
        el.rounded_tl(px(left))
            .rounded_bl(px(left))
            .rounded_tr(px(right))
            .rounded_br(px(right))
            .bg(m3_layer(base, fg, h))
            .text_color(fg),
        fg,
    )
}

/// Una mitad de un split button de M3 Expressive: la acción (`trailing`
/// falso) o la mitad angosta que abre el menú. Esquinas de afuera completas y
/// de adentro chicas (XS); la mitad apretada se cuadra y la del menú, abierta,
/// se vuelve redonda; con `SPATIAL_FAST`. `colors` es su contenedor y su
/// contenido. Quien llama pone las dos en fila con 2 px entre ellas.
#[allow(clippy::too_many_arguments)]
pub fn m3_split<E: gpui::Styled>(
    el: E,
    h: Hover,
    trailing: bool,
    open: bool,
    (base, fg): (Hsla, Hsla),
    height: f32,
    window: &mut Window,
    cx: &mut App,
) -> (E, Hsla) {
    use gpui::px;
    // Adentro, XS (4) de la escala de esquinas.
    let (round, inner) = (height / 2.0, 4.0);
    let (outer, inner) = if m3_pressed(h) {
        (m3_corner_near(height * 0.2), inner)
    } else if trailing && open {
        (round, round)
    } else {
        (round, inner)
    };
    let target = if trailing { [inner, outer] } else { [outer, inner] };
    let [left, right] = gpui_m3::motion::animate("m3-split-radii", gpui_m3::Spring::SPATIAL_FAST, target, window, cx);
    let (left, right) = (left.max(0.0), right.max(0.0));
    (
        el.rounded_tl(px(left))
            .rounded_bl(px(left))
            .rounded_tr(px(right))
            .rounded_br(px(right))
            .bg(m3_layer(base, fg, h))
            .text_color(fg),
        fg,
    )
}

/// Una barra flotante de M3 Expressive: `surface_container`, esquinas
/// completas y elevación 3.
pub fn m3_toolbar(id: impl Into<ElementId>, s: &gpui_m3::Scheme) -> gpui::Stateful<gpui::Div> {
    use gpui::{div, prelude::*, px};
    div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .gap(px(2.))
        .p(px(M3_TOOLBAR_PAD))
        .rounded_full()
        .bg(s.surface_container)
        .shadow(gpui_m3::Elevation::Level3.shadow(s))
}

/// El separador vertical entre grupos de una barra de M3.
pub fn m3_toolbar_divider(s: &gpui_m3::Scheme) -> gpui::Div {
    use gpui::{div, prelude::*, px};
    div().flex_none().w(px(1.)).h(px(24.)).mx(px(4.)).bg(s.outline_variant)
}

/// Un menú o panel que se abre desde una barra de M3: `surface_container`,
/// esquinas grandes y elevación 2.
pub fn m3_menu(id: impl Into<ElementId>, s: &gpui_m3::Scheme) -> gpui::Stateful<gpui::Div> {
    use gpui::{div, prelude::*};
    div()
        .id(id)
        .occlude()
        .rounded(gpui_m3::corner::L)
        .bg(s.surface_container)
        .shadow(gpui_m3::Elevation::Level2.shadow(s))
        .text_color(s.on_surface)
        .font_family(crate::fonts::UI)
}

/// Un aviso sobre la pantalla (la ayuda de la herramienta, «Copiado») como
/// tooltip rico de M3: lo que va antes del primer « · » es el título y lo
/// demás, la explicación. `surface_container_high` con elevación 2.
pub fn m3_hint(s: &gpui_m3::Scheme, text: impl Into<gpui::SharedString>) -> gpui::Div {
    use gpui::{div, prelude::*, px};
    use gpui_m3::theme::type_scale::{TypeScaleExt, BODY_MEDIUM, BODY_SMALL, LABEL_LARGE};
    let text: gpui::SharedString = text.into();
    let (title, rest) = match text.split_once(" · ") {
        Some((title, rest)) => (gpui::SharedString::from(title.to_string()), Some(gpui::SharedString::from(rest.to_string()))),
        None => (text, None),
    };
    div()
        .max_w(px(560.))
        .px(px(12.))
        .py(px(6.))
        .flex()
        .flex_wrap()
        .items_baseline()
        .gap(px(8.))
        .rounded(gpui_m3::corner::M)
        .bg(s.surface_container_high)
        .shadow(gpui_m3::Elevation::Level2.shadow(s))
        .font_family(crate::fonts::UI)
        // Sin título (una frase suelta), el texto va sin énfasis.
        .child(
            div()
                .map(|el| if rest.is_some() { el.type_emphasized(LABEL_LARGE) } else { el.type_style(BODY_MEDIUM) })
                .text_color(s.on_surface)
                .child(title),
        )
        // A lo ancho del globito: así una explicación larga parte en líneas y no se sale.
        .children(rest.map(|rest| div().w_full().type_style(BODY_SMALL).text_color(s.on_surface_variant).child(rest)))
}

/// Un dato corto sobre la pantalla (el tamaño de lo elegido, un color) en un
/// chip de M3: `secondary_container`.
pub fn m3_chip(s: &gpui_m3::Scheme, text: impl Into<gpui::SharedString>) -> gpui::Div {
    use gpui::{div, prelude::*, px};
    use gpui_m3::theme::type_scale::{TypeScaleExt, LABEL_MEDIUM};
    div()
        .h(px(24.))
        .px(px(10.))
        .flex()
        .items_center()
        .rounded(gpui_m3::corner::S)
        .bg(s.secondary_container)
        .text_color(s.on_secondary_container)
        .font_family(crate::fonts::UI)
        .type_style(LABEL_MEDIUM)
        .child(Into::<gpui::SharedString>::into(text))
}

/// Una muestra de color de M3: un círculo que, elegido, se vuelve galleta
/// (Cookie de la biblioteca de Expressive) con un resorte, una vez al cambiar.
/// Con el cursor encima crece un poco. No pide cuadros en reposo.
pub fn m3_swatch(id: impl Into<ElementId>, color: Hsla, selected: bool, size: f32, h: Hover) -> gpui_m3::Shape {
    gpui_m3::Shape::new(gpui_m3::ShapeName::Circle)
        .breathe_on_hover(id, gpui_m3::ShapeName::Cookie9)
        .hovered(selected)
        .size(gpui::px(size))
        .scale(1.0 + 0.08 * h.t - 0.06 * h.press)
        .color(color)
}

/// La entrada de una barra flotante: de 0 a 1 (con el rebote de `SPATIAL`)
/// desde que apareció. Quien llama la corre hacia de donde viene (la pill) en
/// proporción a lo que falta. Con movimiento reducido, 1. Se detiene sola: no
/// pide cuadros en reposo.
pub fn m3_bar_entrance(id: impl Into<ElementId>, window: &mut Window, cx: &mut App) -> f32 {
    if crate::anim::reduced_motion() {
        return 1.0;
    }
    gpui_m3::motion::entrance(id, gpui_m3::Spring::SPATIAL, 0.0, window, cx)
}

/// El globito de los botones de las barras de las herramientas: en
/// Expressive, el tooltip plano de M3 con el esquema de la pill
/// (`inverse_surface`); si no, el de siempre (`tip`).
pub fn tool_tip(text: &'static str) -> impl Fn(&mut Window, &mut App) -> gpui::AnyView + 'static {
    move |_, cx| tool_tip_view(text.into(), cx)
}

/// Como `tool_tip`, con un texto armado en el momento.
pub fn tool_tip_text(text: gpui::SharedString) -> impl Fn(&mut Window, &mut App) -> gpui::AnyView + 'static {
    move |_, cx| tool_tip_view(text.clone(), cx)
}

fn tool_tip_view(text: gpui::SharedString, cx: &mut App) -> gpui::AnyView {
    match crate::pill_look::content_scheme() {
        Some(scheme) => gpui::AppContext::new(cx, |_| TipM3(text, scheme)).into(),
        None => gpui::AppContext::new(cx, |_| Tip(text, None)).into(),
    }
}

/// El tooltip plano de M3 con los colores de la pill.
struct TipM3(gpui::SharedString, gpui_m3::Scheme);

impl gpui::Render for TipM3 {
    fn render(&mut self, _: &mut Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        use gpui::{div, prelude::*, px, Animation, AnimationExt};
        use gpui_m3::theme::type_scale::{TypeScaleExt, BODY_SMALL};
        let s = self.1;
        div().pl(px(10.)).pt(px(8.)).child(
            div()
                .min_h(px(24.))
                .px(px(8.))
                .py(px(4.))
                .rounded(gpui_m3::corner::XS)
                .bg(s.inverse_surface)
                .font_family(crate::fonts::UI)
                .type_style(BODY_SMALL)
                .text_color(s.inverse_on_surface)
                .child(self.0.clone())
                .with_animation(
                    "tip-in-m3-tool",
                    Animation::new(Duration::from_millis(150)).with_easing(gpui::ease_out_quint()),
                    |el, t| el.opacity(t).mt(px(4.0 * (1.0 - t))),
                ),
        )
    }
}
