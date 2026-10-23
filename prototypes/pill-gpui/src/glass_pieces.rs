//! Liquid Glass por piezas (macOS 26): vidrios del sistema bajo la vista de
//! GPUI, uno por panel o control que flota.
//!
//! Un vidrio del tamaño de la ventana se ve como un fondo difuminado: la lente,
//! el brillo y la fusión de Liquid Glass están en los bordes de las piezas. Cada
//! ventana marca dónde va cada pieza (`slot`, un hijo absoluto que llena su
//! elemento) y al final de su raíz (`commit`) se las pasa a AppKit
//! (`platform::window_glass::set_pieces`). Donde hay pieza, GPUI no pinta el
//! fondo del elemento: lo pone lo nativo, y GPUI dibuja encima el texto, los
//! íconos y los estados (hover, selección).
//!
//! Los vidrios van todos juntos en un `NSGlassEffectContainerView`: los que se
//! acercan se funden como gotas y se separan al alejarse, también mientras
//! GPUI los anima. Los rellenos (`Piece::Fill`) quedan debajo de todos los
//! vidrios, en el orden en que se marcan.

use std::cell::RefCell;
use std::collections::HashMap;

use gpui::{Bounds, Hsla, IntoElement, Pixels, Size, Styled, Window, WindowId};

use crate::code::style::Style;

/// Una pieza bajo GPUI.
#[derive(Clone, Copy, PartialEq)]
pub enum Piece {
    /// El vidrio del sistema, con el radio de sus esquinas.
    Glass { radius: f32 },
    /// Un color plano (el fondo de la ventana o de un panel opaco).
    Fill { radius: f32, color: Hsla },
}

type Frame = Vec<(Bounds<Pixels>, Piece)>;

thread_local! {
    /// Las piezas del cuadro en curso, en orden de dibujo.
    static FRAME: RefCell<Frame> = const { RefCell::new(Vec::new()) };
    /// Lo último que se entregó a cada ventana, para no tocar AppKit si no cambió.
    static SENT: RefCell<HashMap<WindowId, (Frame, bool, Size<Pixels>)>> = RefCell::new(HashMap::new());
}

/// Si una ventana con el estilo `style` arma Liquid Glass por piezas: macOS 26
/// en adelante y Liquid Glass.
pub fn active(style: Style) -> bool {
    #[cfg(target_os = "macos")]
    {
        static LIQUID: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        style == Style::Glass && *LIQUID.get_or_init(crate::platform::window_glass::liquid_available)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = style;
        false
    }
}

/// Lo mismo para las ventanas que siguen el estilo de Apariencia (Ajustes,
/// Reuniones, Espacios…).
pub fn active_windows() -> bool {
    crate::theme::styled().is_some_and(|t| active(t.style))
}

/// Marca el lugar de `piece`: va como hijo de un elemento y lo llena. Lo que se
/// marca después queda encima (entre rellenos; los vidrios van sobre todos). Se
/// recorta a lo visible del elemento (scroll, `overflow_hidden`).
pub fn slot(piece: Piece) -> impl IntoElement {
    gpui::canvas(
        move |bounds, window, _| {
            let visible = bounds.intersect(&window.content_mask().bounds);
            if visible.size.width > gpui::px(0.) && visible.size.height > gpui::px(0.) {
                FRAME.with(|frame| frame.borrow_mut().push((visible, piece)));
            }
        },
        |_, _, _, _| {},
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

/// Entrega a la ventana las piezas marcadas en el cuadro: va al final de la
/// raíz, siempre (sin piezas, quita las que hubiera).
pub fn commit(light: bool) -> impl IntoElement {
    commit_with(light, true)
}

fn commit_with(light: bool, base: bool) -> impl IntoElement {
    gpui::canvas(
        move |_, window, _| {
            let pieces = FRAME.with(|frame| std::mem::take(&mut *frame.borrow_mut()));
            let key = (pieces, light, window.viewport_size());
            let id = window.window_handle().window_id();
            if SENT.with(|sent| sent.borrow().get(&id) == Some(&key)) {
                return;
            }
            send(window, &key.0, light, base);
            SENT.with(|sent| sent.borrow_mut().insert(id, key));
        },
        |_, _, _, _| {},
    )
    .absolute()
    .size_0()
}

/// Olvida lo entregado a `window`: el próximo `commit` vuelve a poner las piezas
/// (se cambió el vidrio de la ventana por fuera).
pub fn forget(window: &Window) {
    let id = window.window_handle().window_id();
    SENT.with(|sent| sent.borrow_mut().remove(&id));
}

#[cfg(target_os = "macos")]
fn send(window: &Window, pieces: &[(Bounds<Pixels>, Piece)], light: bool, base: bool) {
    use crate::platform::window_glass::{Piece as Native, PieceKind};
    let Some(view) = crate::platform::win::glass_view(window) else {
        return;
    };
    let native: Vec<Native> = pieces
        .iter()
        .map(|(bounds, piece)| {
            let (radius, kind) = match *piece {
                Piece::Glass { radius } => (radius, PieceKind::Glass),
                Piece::Fill { radius, color } => {
                    let c = gpui::Rgba::from(color);
                    (radius, PieceKind::Fill([c.r as f64, c.g as f64, c.b as f64, c.a as f64]))
                }
            };
            Native {
                x: f32::from(bounds.origin.x) as f64,
                y: f32::from(bounds.origin.y) as f64,
                w: f32::from(bounds.size.width) as f64,
                h: f32::from(bounds.size.height) as f64,
                radius: radius as f64,
                kind,
            }
        })
        .collect();
    crate::platform::window_glass::set_pieces(&view, &native, light, base);
}

#[cfg(not(target_os = "macos"))]
fn send(_: &Window, _: &[(Bounds<Pixels>, Piece)], _: bool, _: bool) {}

/// El fondo de un panel o tarjeta de las ventanas con estilo: un vidrio con
/// Liquid Glass por piezas, y si no `bg`. Va antes de los hijos del elemento.
pub fn panel(el: gpui::Div, radius: f32, bg: Hsla) -> gpui::Div {
    use gpui::ParentElement;
    if active_windows() {
        el.child(slot(Piece::Glass { radius }))
    } else {
        el.bg(bg)
    }
}

/// El fondo de la raíz de una ventana con estilo: con piezas, un relleno nativo
/// (si GPUI lo pintara, taparía los vidrios).
pub fn root(el: gpui::Div, bg: Hsla) -> gpui::Div {
    use gpui::ParentElement;
    if active_windows() {
        el.child(slot(Piece::Fill { radius: 0., color: bg }))
    } else {
        el.bg(bg)
    }
}

/// Un vidrio detrás de un control que pinta su propio contenido (los de
/// gpui-m3): `el` lo envuelve y el vidrio va con el radio `radius`. Sin piezas,
/// `el` queda igual. Va antes de los hijos.
pub fn behind<E: gpui::ParentElement + Styled>(el: E, radius: f32) -> E {
    if active_windows() {
        el.relative().rounded(gpui::px(radius)).child(slot(Piece::Glass { radius }))
    } else {
        el
    }
}

// --- Transformaciones: un vidrio que nace de un control -------------------------
//
// Al abrirse, un panel o menú sale del botón que lo abre: su vidrio parte con
// la forma del botón y se estira con un resorte hasta la suya; al cerrarse,
// vuelve a meterse en el botón. Al salir y al volver se funde con el vidrio del
// botón (van en el mismo contenedor). El contenido aparece cuando el vidrio
// ya casi llegó (`Morph::fade`).

/// Lo que dura la transformación.
const MORPH_SECS: f32 = 0.42;

/// El estado de una transformación, compartido entre el ancla (el botón), el
/// destino (el panel) y la raíz (`morph_ghost`).
#[derive(Default)]
pub struct Morph {
    open: bool,
    /// Qué control la abrió (cada uno marca su ancla con su clave).
    key: u64,
    anchors: HashMap<u64, (Bounds<Pixels>, f32)>,
    target: Option<Bounds<Pixels>>,
    started: Option<std::time::Instant>,
    /// El destino se marcó en este cuadro (si no, la raíz dibuja el fantasma).
    seen: bool,
}

pub type MorphState = std::rc::Rc<RefCell<Morph>>;

impl Morph {
    /// Abre o cierra desde el control `key`. Sin cambio, nada.
    pub fn set_open(&mut self, open: bool, key: u64) {
        if open == self.open {
            return;
        }
        self.open = open;
        if open {
            self.key = key;
        }
        self.started = Some(std::time::Instant::now());
    }

    /// Un ancla que no es un elemento (el punto del clic).
    pub fn set_anchor(&mut self, key: u64, bounds: Bounds<Pixels>, radius: f32) {
        self.anchors.insert(key, (bounds, radius));
    }

    /// Cuánto está abierto, de 0 (el botón) a 1, mientras se transforma.
    fn openness(&self) -> Option<f32> {
        let elapsed = self.started?.elapsed().as_secs_f32();
        if elapsed >= MORPH_SECS {
            return None;
        }
        let eased = spring(elapsed / MORPH_SECS);
        Some(if self.open { eased } else { 1. - eased })
    }

    /// La opacidad del contenido: aparece cuando el vidrio ya casi llegó.
    pub fn fade(&self) -> f32 {
        match self.openness() {
            Some(open) if self.open => ((open - 0.45) / 0.55).clamp(0., 1.),
            Some(open) => (open * 2.).clamp(0., 1.),
            None => 1.,
        }
    }

    /// El vidrio de ahora: entre el ancla y el destino.
    fn piece(&self, target: Bounds<Pixels>, radius: f32) -> Option<(Bounds<Pixels>, Piece, bool)> {
        let Some(open) = self.openness() else {
            return self.open.then_some((target, Piece::Glass { radius }, false));
        };
        let Some(&(anchor, anchor_radius)) = self.anchors.get(&self.key) else {
            return self.open.then_some((target, Piece::Glass { radius }, false));
        };
        let lerp = |a: f32, b: f32| a + (b - a) * open;
        let bounds = Bounds::new(
            gpui::point(
                gpui::px(lerp(f32::from(anchor.origin.x), f32::from(target.origin.x))),
                gpui::px(lerp(f32::from(anchor.origin.y), f32::from(target.origin.y))),
            ),
            gpui::size(
                gpui::px(lerp(f32::from(anchor.size.width), f32::from(target.size.width)).max(1.)),
                gpui::px(lerp(f32::from(anchor.size.height), f32::from(target.size.height)).max(1.)),
            ),
        );
        let radius = lerp(anchor_radius, radius).max(0.);
        Some((bounds, Piece::Glass { radius }, true))
    }
}

/// Un resorte con un poco de rebote al final, como los de Liquid Glass.
fn spring(t: f32) -> f32 {
    let t = t.clamp(0., 1.);
    let (c1, c3) = (1.3, 2.3);
    1. + c3 * (t - 1.).powi(3) + c1 * (t - 1.).powi(2)
}

/// Marca el ancla de `state` con la clave `key`: va dentro del botón que abre.
pub fn morph_anchor(state: MorphState, key: u64, radius: f32) -> impl IntoElement {
    gpui::canvas(move |bounds, _, _| state.borrow_mut().set_anchor(key, bounds, radius), |_, _, _, _| {})
        .absolute()
        .top_0()
        .left_0()
        .size_full()
}

/// El vidrio del destino (el panel, el menú), que sale del ancla al abrirse y
/// vuelve a ella al cerrarse. Como `slot`, va antes de los hijos.
pub fn morph_slot(state: MorphState, radius: f32) -> impl IntoElement {
    gpui::canvas(
        move |bounds, window, _| {
            let mut morph = state.borrow_mut();
            morph.target = Some(bounds);
            morph.seen = true;
            if let Some((bounds, piece, moving)) = morph.piece(bounds, radius) {
                FRAME.with(|frame| frame.borrow_mut().push((bounds, piece)));
                if moving {
                    window.request_animation_frame();
                }
            }
        },
        |_, _, _, _| {},
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

/// Al final de la raíz: si el destino ya no se dibuja (se cerró de golpe), su
/// vidrio sigue volviendo al ancla.
pub fn morph_ghost(state: MorphState, radius: f32) -> impl IntoElement {
    gpui::canvas(
        move |_, window, _| {
            let mut morph = state.borrow_mut();
            let seen = std::mem::take(&mut morph.seen);
            if seen || morph.open {
                return;
            }
            let Some(target) = morph.target else {
                return;
            };
            if let Some((bounds, piece, true)) = morph.piece(target, radius) {
                FRAME.with(|frame| frame.borrow_mut().push((bounds, piece)));
                window.request_animation_frame();
            }
        },
        |_, _, _, _| {},
    )
    .absolute()
    .size_0()
}
