//! Un indicador de selección que se desliza entre las opciones en vez de saltar,
//! como las barras de pestañas de Liquid Glass: el borde que va adelante corre
//! y el de atrás lo sigue con un resorte, así que en el camino la píldora se
//! estira en la dirección del movimiento y se angosta un poco en la otra; al
//! llegar se recoge con un leve rebote.
//!
//! Cada opción marca su lugar (`mark`, con su clave) y el contenedor dibuja el
//! indicador (`indicator`) antes de sus hijos, para que quede debajo del texto.
//! Las marcas se toman en el prepaint y el indicador pinta después, en el paint,
//! así que usa las posiciones del mismo cuadro. Las posiciones son relativas al
//! contenedor: un scroll mueve todo junto sin animar.
//!
//! Sin opción elegida (un interruptor que se apaga), la píldora se recoge hacia
//! su centro y se desvanece; al volver a elegir, nace ahí mismo y se estira.
//!
//! En una lista con scroll, cada opción sabe cuál es su recorte
//! (`content_mask`) y la píldora no se sale de él; si la opción elegida solo se
//! movió (scroll, cambio de tamaño), la píldora la sigue sin animar. Con
//! `mark_on` y `indicator_marked`, la opción elegida la dice cada opción al
//! dibujarse, para listas armadas en varias funciones.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Instant;

use gpui::{Bounds, Hsla, IntoElement, Pixels, Styled, Window};

/// Lo que tarda el borde de atrás en llegar; el de adelante, `LEAD` de eso.
const SECS: f32 = 0.5;
const LEAD: f32 = 0.55;
/// Cuánto se angosta, a lo más, en el eje que no se mueve (en px).
const SQUEEZE: f32 = 3.;
/// Lo que tarda en aparecer o desvanecerse.
const FADE_SECS: f32 = 0.18;

/// Un rectángulo en px relativos al contenedor: izquierda, arriba, derecha, abajo.
type Edges = [f32; 4];

#[derive(Clone, Copy)]
struct Mark {
    edges: Edges,
    /// Lo visible de la opción (su recorte), relativo al contenedor.
    clip: Edges,
    radius: Option<f32>,
}

#[derive(Default)]
pub struct Glide {
    marks: HashMap<u64, Mark>,
    /// La opción a la que va, y el recorte de la anterior mientras viaja.
    key: Option<u64>,
    clip_from: Option<Edges>,
    /// La que se marcó como elegida en este cuadro (`mark_on`).
    chosen: Option<u64>,
    /// Hacia dónde va y desde dónde partió.
    to: Option<Edges>,
    from: Edges,
    started: Option<Instant>,
    /// El origen del contenedor en este cuadro.
    origin: Option<gpui::Point<Pixels>>,
    /// Si hay opción elegida, y desde cuándo y qué opacidad cambió eso.
    shown: bool,
    fade_from: f32,
    fade_started: Option<Instant>,
    /// Se dibujó sin opción elegida antes de la primera: la primera nace animada.
    born_hidden: bool,
}

pub type GlideState = Rc<RefCell<Glide>>;

pub fn state() -> GlideState {
    Rc::new(RefCell::new(Glide::default()))
}

thread_local! {
    static NAMED: RefCell<HashMap<gpui::SharedString, GlideState>> = RefCell::new(HashMap::new());
}

/// El estado guardado con el nombre `key`, para las vistas que no lo llevan en
/// su estructura. El nombre distingue cada fila de opciones (y cada panel, si
/// la misma fila se repite).
pub fn named(key: impl Into<gpui::SharedString>) -> GlideState {
    let key = key.into();
    NAMED.with(|named| named.borrow_mut().entry(key).or_insert_with(state).clone())
}

/// `edges` recogido hacia su centro, de donde nace y adonde vuelve.
fn shrunk(edges: Edges) -> Edges {
    let (cx, cy) = ((edges[0] + edges[2]) / 2., (edges[1] + edges[3]) / 2.);
    let (w, h) = ((edges[2] - edges[0]) * 0.3, (edges[3] - edges[1]) * 0.5);
    [cx - w / 2., cy - h / 2., cx + w / 2., cy + h / 2.]
}

impl Glide {
    fn alpha(&self, now: Instant) -> f32 {
        let Some(started) = self.fade_started else {
            return if self.shown { 1. } else { 0. };
        };
        let t = (now.saturating_duration_since(started).as_secs_f32() / FADE_SECS).min(1.);
        let to = if self.shown { 1. } else { 0. };
        crate::anim::lerp(self.fade_from, to, t)
    }

    fn set_shown(&mut self, shown: bool, now: Instant) {
        if shown != self.shown {
            self.fade_from = self.alpha(now);
            self.shown = shown;
            self.fade_started = Some(now);
        }
    }

    /// Dónde va el indicador ahora y con qué opacidad; `true` si se sigue
    /// moviendo. Sin `target`, se recoge donde estaba.
    fn current(&mut self, key: Option<u64>, target: Option<Edges>) -> Option<(Edges, f32, bool)> {
        let same = key.is_some() && key == self.key;
        if key.is_some() && !same {
            self.clip_from = self.key.and_then(|old| self.marks.get(&old)).map(|mark| mark.clip);
            self.key = key;
        }
        // La misma opción, movida (scroll, tamaño): la píldora la sigue sin
        // reiniciar el viaje.
        if let (true, Some(target), Some(to)) = (same, target, self.to) {
            if self.shown && to != target {
                for edge in 0..4 {
                    self.from[edge] += target[edge] - to[edge];
                }
                self.to = Some(target);
            }
        }
        self.current_to(target)
    }

    fn current_to(&mut self, target: Option<Edges>) -> Option<(Edges, f32, bool)> {
        let now = Instant::now();
        let reduced = crate::code::style::system_reduced_motion();
        let Some(to) = self.to else {
            let Some(target) = target else {
                self.born_hidden = true;
                return None;
            };
            self.to = Some(target);
            if self.born_hidden && !reduced {
                // Se elige por primera vez: nace en su centro.
                self.from = shrunk(target);
                self.started = Some(now);
                self.set_shown(true, now);
                return self.current_to(Some(target));
            }
            // Ya elegida al abrirse: aparece en su sitio, sin animar.
            self.from = target;
            self.shown = true;
            return Some((target, 1., false));
        };
        let target = match target {
            Some(target) => {
                if !self.shown {
                    // Nace en el centro de la elegida.
                    self.from = shrunk(target);
                    self.to = Some(target);
                    self.started = (!reduced).then_some(now);
                }
                self.set_shown(true, now);
                target
            }
            None => {
                if self.shown {
                    self.set_shown(false, now);
                    let target = shrunk(to);
                    self.from = self.at(to, now).0;
                    self.to = Some(target);
                    self.started = (!reduced).then_some(now);
                }
                self.to.unwrap_or(to)
            }
        };
        let to = self.to.unwrap_or(target);
        if to != target {
            // Cambió la selección (o se redirige a mitad de camino): parte de
            // donde está.
            self.from = self.at(to, now).0;
            self.to = Some(target);
            self.started = Some(now);
            if reduced {
                self.from = target;
                self.started = None;
            }
        }
        let alpha = self.alpha(now);
        let (edges, moving) = self.at(target, now);
        let fading = self.fade_started.is_some_and(|started| now.saturating_duration_since(started).as_secs_f32() < FADE_SECS);
        Some((edges, alpha, moving || fading))
    }

    fn at(&self, to: Edges, now: Instant) -> (Edges, bool) {
        let Some(started) = self.started else {
            return (to, false);
        };
        let t = now.saturating_duration_since(started).as_secs_f32() / SECS;
        if t >= 1. {
            return (to, false);
        }
        let from = self.from;
        let lead = crate::anim::ease_smooth_out((t / LEAD).min(1.));
        let trail = spring(t);
        let mut edges = [0.; 4];
        // En cada eje, el borde que va en la dirección del movimiento adelanta.
        for axis in 0..2 {
            let (low, high) = (axis, axis + 2);
            let forward = (to[low] + to[high]) >= (from[low] + from[high]);
            let (p_low, p_high) = if forward { (trail, lead) } else { (lead, trail) };
            edges[low] = crate::anim::lerp(from[low], to[low], p_low);
            edges[high] = crate::anim::lerp(from[high], to[high], p_high);
        }
        // Mientras se estira, se angosta en el otro eje: el líquido conserva su
        // volumen.
        let moved_x = ((to[0] + to[2]) - (from[0] + from[2])).abs();
        let moved_y = ((to[1] + to[3]) - (from[1] + from[3])).abs();
        let stretch_y = (edges[3] - edges[1]) - crate::anim::lerp(from[3] - from[1], to[3] - to[1], trail);
        let stretch_x = (edges[2] - edges[0]) - crate::anim::lerp(from[2] - from[0], to[2] - to[0], trail);
        if moved_y >= moved_x {
            let inset = (stretch_y.abs() * 0.12).min(SQUEEZE);
            edges[0] += inset;
            edges[2] -= inset;
        } else {
            let inset = (stretch_x.abs() * 0.12).min(SQUEEZE);
            edges[1] += inset;
            edges[3] -= inset;
        }
        (edges, true)
    }
}

/// Un resorte con un poco de rebote al final.
fn spring(t: f32) -> f32 {
    let t = t.clamp(0., 1.);
    let (c1, c3) = (1.2, 2.2);
    1. + c3 * (t - 1.).powi(3) + c1 * (t - 1.).powi(2)
}

/// Marca el lugar de la opción `key`: va dentro de ella, como hijo.
pub fn mark(state: GlideState, key: u64) -> impl IntoElement {
    mark_on(state, key, false, None)
}

/// Como `mark`, y si `on`, la opción elegida para `indicator_marked`; `radius`,
/// si la opción tiene otro radio que el del indicador.
pub fn mark_on(state: GlideState, key: u64, on: bool, radius: Option<f32>) -> impl IntoElement {
    gpui::canvas(
        move |bounds: Bounds<Pixels>, window, _| {
            let mut glide = state.borrow_mut();
            let Some(origin) = glide.origin else {
                return;
            };
            let relative = |bounds: Bounds<Pixels>| {
                let left = f32::from(bounds.origin.x - origin.x);
                let top = f32::from(bounds.origin.y - origin.y);
                [left, top, left + f32::from(bounds.size.width), top + f32::from(bounds.size.height)]
            };
            let clip = relative(window.content_mask().bounds);
            glide.marks.insert(key, Mark { edges: relative(bounds), clip, radius });
            if on {
                glide.chosen = Some(key);
            }
        },
        |_, _, _, _| {},
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

/// Cómo se ve el indicador.
#[derive(Clone, Copy)]
pub struct Look {
    pub fill: Hsla,
    pub radius: f32,
    /// Un borde fino (el brillo del vidrio), si hay.
    pub edge: Option<Hsla>,
}

/// El indicador bajo la opción `selected`: va en el contenedor de las opciones,
/// antes de ellas. Sin marca para `selected` (aún no se dibuja), nada; con
/// `None`, se recoge y se desvanece.
pub fn indicator(state: GlideState, selected: Option<u64>, look: Look) -> impl IntoElement {
    paint_indicator(state, Some(selected), look)
}

/// El indicador bajo la opción que se marcó elegida en este cuadro
/// (`mark_on`); si ninguna, se recoge.
pub fn indicator_marked(state: GlideState, look: Look) -> impl IntoElement {
    paint_indicator(state, None, look)
}

/// `selected`: `None` es «la que diga `mark_on`».
fn paint_indicator(state: GlideState, selected: Option<Option<u64>>, look: Look) -> impl IntoElement {
    let prepaint_state = state.clone();
    gpui::canvas(
        move |bounds: Bounds<Pixels>, _, _| {
            let mut glide = prepaint_state.borrow_mut();
            glide.origin = Some(bounds.origin);
            glide.chosen = None;
        },
        move |bounds: Bounds<Pixels>, _, window: &mut Window, _| {
            let mut glide = state.borrow_mut();
            let selected = selected.unwrap_or(glide.chosen);
            let mark = match selected {
                Some(key) => match glide.marks.get(&key) {
                    Some(&mark) => Some(mark),
                    None => return,
                },
                None => None,
            };
            let Some((edges, alpha, moving)) = glide.current(selected, mark.map(|mark| mark.edges)) else {
                return;
            };
            // Mientras viaja, el recorte abarca el de la opción de antes y el de la
            // nueva; quieta, solo el suyo.
            let clip_to = mark.or_else(|| glide.key.and_then(|key| glide.marks.get(&key)).copied()).map(|mark| mark.clip);
            let clip = match (clip_to, glide.clip_from) {
                (Some(to), Some(from)) if moving => Some([to[0].min(from[0]), to[1].min(from[1]), to[2].max(from[2]), to[3].max(from[3])]),
                (to, _) => to,
            };
            let radius = mark.and_then(|mark| mark.radius).unwrap_or(look.radius);
            if moving {
                window.request_animation_frame();
            }
            if alpha <= 0. {
                return;
            }
            let quad = Bounds::from_corners(
                gpui::point(bounds.origin.x + gpui::px(edges[0]), bounds.origin.y + gpui::px(edges[1])),
                gpui::point(bounds.origin.x + gpui::px(edges[2]), bounds.origin.y + gpui::px(edges[3])),
            );
            let radius = radius.min(f32::from(quad.size.height) / 2.).min(f32::from(quad.size.width) / 2.);
            let mut quad = gpui::fill(quad, look.fill.opacity(look.fill.a * alpha)).corner_radii(gpui::px(radius));
            if let Some(edge) = look.edge {
                quad = quad.border_widths(gpui::px(1.)).border_color(edge.opacity(edge.a * alpha));
            }
            let mask = clip.map(|clip| gpui::ContentMask {
                bounds: Bounds::from_corners(
                    gpui::point(bounds.origin.x + gpui::px(clip[0]), bounds.origin.y + gpui::px(clip[1])),
                    gpui::point(bounds.origin.x + gpui::px(clip[2]), bounds.origin.y + gpui::px(clip[3])),
                ),
            });
            window.with_content_mask(mask, |window| window.paint_quad(quad));
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}
