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
    use gpui::{div, prelude::*, px, rgb, svg};
    let ink: Hsla = rgb(0x1a1a18).into();
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
            .rounded(px(if active { 8. } else { 13. }))
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
    fn label(&self, color: Hsla) -> gpui::Div {
        use gpui::{div, prelude::*, px};
        div().flex().items_center().gap(px(6.)).child(self.0.clone()).children(self.1.clone().map(|key| {
            div().px(px(4.)).rounded(px(4.)).bg(color.opacity(0.18)).text_size(px(10.)).child(key)
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
                        .child(self.label(t.pane))
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
                    .child(self.label(rgb(0xf0f0ea).into()))
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
/// filas, y casi cuadradas al apretar.
pub const M3_GROUP_RADII: (f32, f32, f32) = (16.0, 4.0, 2.0);

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
                .child(svg().path(icon).size(px(24.)).text_color(s.on_secondary_container)),
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
