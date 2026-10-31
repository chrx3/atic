//! Controles nativos de macOS dentro de una vista de GPUI, como los de
//! `platform/macos/segmented.rs`: GPUI les guarda el lugar (un `div` del tamaño
//! que pide AppKit), marca dónde quedó en el cuadro y al final de la raíz
//! (`commit`) AppKit los pone ahí, encima de GPUI. Lo que no se marcó en el
//! cuadro se esconde.
//!
//! Van encima de todo lo de GPUI: si la ventana abre algo que los tapa (un
//! menú, un diálogo), `commit(true)` los esconde mientras tanto.

#[cfg(target_os = "macos")]
pub use imp::*;

#[cfg(not(target_os = "macos"))]
pub fn commit(_covered: bool) -> impl gpui::IntoElement {
    gpui::div()
}

/// Fuera de macOS no hay control nativo: quien lo usa lo elige con
/// `cfg!(target_os = "macos")`, pero el código igual tiene que compilar.
#[cfg(not(target_os = "macos"))]
#[allow(dead_code)]
pub struct Segment {
    pub label: String,
    pub symbol: &'static str,
    pub tip: String,
    pub selected: bool,
}

#[cfg(not(target_os = "macos"))]
pub fn segmented<V: 'static>(
    _key: &str,
    _segments: Vec<Segment>,
    _icon_only: bool,
    _light: bool,
    _window: &mut gpui::Window,
    _cx: &mut gpui::Context<V>,
    _on_pick: impl Fn(&mut V, usize, &mut gpui::Window, &mut gpui::Context<V>) + 'static,
) -> gpui::Div {
    gpui::div()
}

#[cfg(target_os = "macos")]
mod imp {
    use std::cell::RefCell;
    use std::collections::HashMap;

    use futures::StreamExt;
    use gpui::{canvas, div, px, Bounds, Context, Div, IntoElement, ParentElement, Pixels, Styled, Window, WindowId};

    pub use crate::platform::segmented::Segment;
    use crate::platform::segmented;

    thread_local! {
        /// El control de cada llave en cada ventana.
        static TAGS: RefCell<HashMap<(WindowId, String), isize>> = RefCell::new(HashMap::new());
        /// Dónde quedó cada control en el cuadro en curso.
        static FRAME: RefCell<Vec<(WindowId, isize, Bounds<Pixels>)>> = const { RefCell::new(Vec::new()) };
        /// Lo último que se le pidió a AppKit para cada control.
        static PLACED: RefCell<HashMap<isize, Option<Bounds<Pixels>>>> = RefCell::new(HashMap::new());
    }

    /// Un `NSSegmentedControl` con `segments`, en un `div` de su tamaño. El clic
    /// llama a `on_pick` con el índice del segmento que cambió; se toma la primera
    /// vez que aparece `key` en la ventana (no captures nada del cuadro).
    pub fn segmented<V: 'static>(
        key: &str,
        segments: Vec<Segment>,
        icon_only: bool,
        light: bool,
        window: &mut Window,
        cx: &mut Context<V>,
        on_pick: impl Fn(&mut V, usize, &mut Window, &mut Context<V>) + 'static,
    ) -> Div {
        let id = window.window_handle().window_id();
        let tag = TAGS.with(|tags| tags.borrow().get(&(id, key.to_string())).copied());
        let tag = tag.unwrap_or_else(|| {
            let (tx, mut rx) = futures::channel::mpsc::unbounded();
            let tag = segmented::create(tx);
            TAGS.with(|tags| tags.borrow_mut().insert((id, key.to_string()), tag));
            cx.spawn_in(window, async move |this, cx| {
                while let Some(index) = rx.next().await {
                    if this.update_in(cx, |view, window, cx| on_pick(view, index, window, cx)).is_err() {
                        break;
                    }
                }
            })
            .detach();
            tag
        });
        let (w, h) = segmented::update(tag, &segments, icon_only, light);
        div().w(px(w)).h(px(h)).flex_none().relative().child(
            canvas(
                move |bounds, window, _| {
                    // Un control no se recorta: si no se ve entero, no va.
                    if window.content_mask().bounds.intersect(&bounds) == bounds {
                        FRAME.with(|frame| frame.borrow_mut().push((window.window_handle().window_id(), tag, bounds)));
                    }
                },
                |_, _, _, _| {},
            )
            .absolute()
            .top_0()
            .left_0()
            .size_full(),
        )
    }

    /// Pone los controles marcados en el cuadro y esconde el resto: va al final
    /// de la raíz, siempre. `covered`: algo de GPUI los tapa ahora, se esconden.
    pub fn commit(covered: bool) -> impl IntoElement {
        canvas(
            move |_, window, _| {
                let id = window.window_handle().window_id();
                let marked: Vec<(isize, Bounds<Pixels>)> = FRAME.with(|frame| {
                    let mut frame = frame.borrow_mut();
                    let (mine, rest): (Vec<_>, Vec<_>) = frame.drain(..).partition(|(window, ..)| *window == id);
                    *frame = rest;
                    mine.into_iter().map(|(_, tag, bounds)| (tag, bounds)).collect()
                });
                let tags: Vec<isize> =
                    TAGS.with(|tags| tags.borrow().iter().filter(|((window, _), _)| *window == id).map(|(_, tag)| *tag).collect());
                if tags.is_empty() {
                    return;
                }
                let Some(view) = crate::platform::win::glass_view(window) else {
                    return;
                };
                for tag in tags {
                    let want = (!covered).then(|| marked.iter().find(|(t, _)| *t == tag).map(|(_, b)| *b)).flatten();
                    if PLACED.with(|placed| placed.borrow().get(&tag) == Some(&want)) {
                        continue;
                    }
                    let rect = want.map(|b| {
                        (f32::from(b.origin.x), f32::from(b.origin.y), f32::from(b.size.width), f32::from(b.size.height))
                    });
                    segmented::place(tag, &view, rect);
                    PLACED.with(|placed| placed.borrow_mut().insert(tag, want));
                }
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_0()
    }
}
