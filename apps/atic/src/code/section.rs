//! La barra lateral, el chat y el panel derecho como vistas propias que GPUI cachea.
//!
//! Atic Code es una sola vista (`CodeView`): cualquier aviso (el texto que llega, un
//! indicador que gira, el cursor que parpadea) rehacía el layout y el pintado de toda la
//! ventana. Cada `Section` dibuja una parte de `CodeView` (llama a su método con la vista
//! prestada) y va en la raíz con `AnyView::cached`: GPUI reutiliza su último cuadro mientras
//! no cambie de tamaño y no haya cambiado nada de lo que leyó al dibujarse. `CodeView` es
//! una de esas cosas, así que un `cx.notify()` de la vista rehace las tres secciones como
//! antes; un aviso a una sola sección (el streaming al chat, ver `redraw_on_tick`) o a la
//! capa de indicadores (`spinners.rs`) no toca las demás. (Para eso el GPUI de `vendor/`
//! anota qué entidades lee cada vista cacheada y cuáles avisaron en el cuadro.)
//!
//! Lo que una sección anota en el cuadro para la raíz (las piezas de Liquid Glass, los
//! controles nativos, los huecos de los indicadores) se anota en el prepaint de sus elementos; en
//! un cuadro reutilizado eso no pasa, así que la sección guarda lo que anotó la última vez
//! y la raíz lo repite en su lugar (`element`).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gpui::{
    canvas, div, AnyElement, AnyView, AppContext, Context, Entity, IntoElement, ParentElement, Render, StyleRefinement, Styled,
    WeakEntity, Window,
};

use super::CodeView;

/// Qué parte de `CodeView` dibuja una sección.
pub(super) type Part = fn(&mut CodeView, &mut Window, &mut Context<CodeView>) -> AnyElement;

pub(super) struct Section {
    owner: WeakEntity<CodeView>,
    part: Part,
    marks: Rc<Marks>,
}

/// Lo que la sección anotó para la raíz en su último dibujo.
#[derive(Default)]
struct Marks {
    /// Se dibujó en este cuadro: no hay que repetir nada.
    fresh: Cell<bool>,
    /// Dónde iban las listas del cuadro al empezar la sección.
    start: Cell<(usize, usize, usize)>,
    glass: RefCell<crate::glass_pieces::Frame>,
    controls: RefCell<crate::native_controls::Marked>,
    spinners: RefCell<Vec<super::spinners::Spot>>,
}

impl Section {
    pub(super) fn new(owner: WeakEntity<CodeView>, part: Part) -> Self {
        Self { owner, part, marks: Rc::default() }
    }
}

impl Render for Section {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let part = self.part;
        let content = self.owner.update(cx, |view, cx| part(view, window, cx)).unwrap_or_else(|_| div().into_any_element());
        let (begin, end) = (self.marks.clone(), self.marks.clone());
        // Como hija única de una fila flex, igual que cuando colgaba de la raíz.
        div()
            .size_full()
            .flex()
            .child(
                canvas(
                    move |_, _, _| begin.start.set((crate::glass_pieces::mark(), crate::native_controls::mark(), super::spinners::mark())),
                    |_, _, _, _| {},
                )
                .absolute()
                .size_0(),
            )
            .child(content)
            .child(
                canvas(
                    move |_, _, _| {
                        let (glass, controls, spinners) = end.start.get();
                        *end.glass.borrow_mut() = crate::glass_pieces::since(glass);
                        *end.controls.borrow_mut() = crate::native_controls::since(controls);
                        *end.spinners.borrow_mut() = super::spinners::since(spinners);
                        end.fresh.set(true);
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_0(),
            )
    }
}

/// La sección `entity` como hija de la raíz, con la caja de `style` (GPUI no la mide
/// mientras la reutiliza: la caja tiene que salir de afuera), y detrás lo que anotó la
/// última vez que se dibujó, si en este cuadro se reutilizó.
pub(super) fn element(entity: &Entity<Section>, style: StyleRefinement, cx: &gpui::App) -> [AnyElement; 2] {
    let marks = entity.read(cx).marks.clone();
    let replay = canvas(
        move |_, _, _| {
            if !marks.fresh.replace(false) {
                crate::glass_pieces::replay(&marks.glass.borrow());
                crate::native_controls::replay(&marks.controls.borrow());
                super::spinners::replay(&marks.spinners.borrow());
            }
        },
        |_, _, _, _| {},
    )
    .absolute()
    .size_0();
    [AnyView::from(entity.clone()).cached(style).into_any_element(), replay.into_any_element()]
}

/// Las secciones de una ventana de Atic Code y su capa de indicadores.
#[derive(Clone)]
pub(super) struct Sections {
    pub sidebar: Entity<Section>,
    pub chat: Entity<Section>,
    pub right: Entity<Section>,
    pub spinners: Entity<super::spinners::SpinnerLayer>,
}

impl Sections {
    pub(super) fn new(sidebar: Part, chat: Part, right: Part, cx: &mut Context<CodeView>) -> Self {
        let owner = cx.weak_entity();
        Self {
            sidebar: cx.new(|_| Section::new(owner.clone(), sidebar)),
            chat: cx.new(|_| Section::new(owner.clone(), chat)),
            right: cx.new(|_| Section::new(owner, right)),
            spinners: cx.new(|_| super::spinners::SpinnerLayer),
        }
    }
}
