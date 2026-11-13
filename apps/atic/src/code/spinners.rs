//! Los indicadores de carga de Atic Code (el `LoadingIndicator` de M3) dibujados en una
//! capa propia, encima de la barra, del hilo y del panel derecho.
//!
//! Un indicador anima a 15 cuadros por segundo y cada cuadro le avisa a la vista que lo
//! dibuja; GPUI rehace esa vista y todas las que la contienen. Dentro de la barra o del
//! hilo, eso era rehacer la sección entera (`section.rs`) solo para girar una figura de
//! 18 px. Aquí la fila deja un hueco del mismo tamaño (`slot`) que anota dónde quedó, y
//! `SpinnerLayer` (una vista hoja, después de las secciones) dibuja ahí la figura: la
//! animación solo rehace esta capa, y las secciones quedan cacheadas.
//!
//! El hueco anota también el recorte de lo que lo contiene (el scroll de la barra o del
//! hilo) y la opacidad con que se pintó (una fila que entra fundiéndose, el «Trabajando…»
//! atenuado), para que la figura se vea igual que si estuviera dentro.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gpui::{
    canvas, div, px, AnyElement, Bounds, ContentMask, Context, IntoElement, ParentElement, Pixels, Render, Styled, Window, WindowId,
};

/// Un hueco anotado en el cuadro en curso.
#[derive(Clone)]
pub(super) struct Spot {
    window: WindowId,
    bounds: Bounds<Pixels>,
    mask: ContentMask<Pixels>,
    size: f32,
    /// La opacidad con que se pintó el hueco: se sabe al pintar, después de que la capa
    /// arma sus figuras, y se comparte con la copia que guarda una sección cacheada.
    opacity: Rc<Cell<f32>>,
}

thread_local! {
    /// Los huecos del cuadro en curso, en orden de dibujo.
    static FRAME: RefCell<Vec<Spot>> = const { RefCell::new(Vec::new()) };
}

/// El lugar de un indicador de `size` px: mide lo mismo que el `LoadingIndicator` y lo
/// dibuja `SpinnerLayer`.
pub(super) fn slot(size: f32) -> impl IntoElement {
    let opacity = Rc::new(Cell::new(1.));
    let painted = opacity.clone();
    div().relative().flex_none().size(px(size)).child(
        canvas(
            move |bounds, window, _| {
                let mask = window.content_mask();
                // Fuera de lo visible (scroll) no hay nada que dibujar.
                let visible = bounds.intersect(&mask.bounds).size;
                if visible.width > px(0.) && visible.height > px(0.) {
                    let window = window.window_handle().window_id();
                    FRAME.with(|frame| frame.borrow_mut().push(Spot { window, bounds, mask, size, opacity }));
                }
            },
            move |_, _, window, _| painted.set(window.element_opacity()),
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full(),
    )
}

/// Cuántos huecos lleva anotados el cuadro (ver `glass_pieces::mark`).
pub(super) fn mark() -> usize {
    FRAME.with(|frame| frame.borrow().len())
}

/// Los huecos anotados desde `start`.
pub(super) fn since(start: usize) -> Vec<Spot> {
    FRAME.with(|frame| frame.borrow().get(start..).map(<[_]>::to_vec).unwrap_or_default())
}

/// Vuelve a anotar `spots` en el cuadro en curso.
pub(super) fn replay(spots: &[Spot]) {
    FRAME.with(|frame| frame.borrow_mut().extend_from_slice(spots));
}

/// La capa que dibuja los indicadores de los huecos. Va en la raíz de la ventana después
/// de las secciones y antes de los menús y diálogos, que la tapan como tapaban a la fila.
pub(super) struct SpinnerLayer;

impl Render for SpinnerLayer {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        canvas(
            |_, window, cx| {
                let id = window.window_handle().window_id();
                let spots: Vec<Spot> = FRAME.with(|frame| {
                    let mut frame = frame.borrow_mut();
                    let (mine, rest): (Vec<_>, Vec<_>) = frame.drain(..).partition(|spot| spot.window == id);
                    *frame = rest;
                    mine
                });
                // La figura se arma aquí, con la vista de la capa como la actual: su
                // animación (`motion::continuous_at`) le avisa a esta capa y no a la ventana.
                spots
                    .into_iter()
                    .map(|spot| {
                        let mut figure: AnyElement = gpui_m3::LoadingIndicator::new().size(px(spot.size)).into_any_element();
                        window.with_content_mask(Some(spot.mask.clone()), |window| {
                            figure.prepaint_as_root(spot.bounds.origin, spot.bounds.size.into(), window, cx);
                        });
                        (figure, spot)
                    })
                    .collect::<Vec<_>>()
            },
            |_, mut figures, window, cx| {
                for (figure, spot) in &mut figures {
                    window.with_content_mask(Some(spot.mask.clone()), |window| {
                        window.with_element_opacity(Some(spot.opacity.get()), |window| figure.paint(window, cx));
                    });
                }
            },
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full()
    }
}
