//! El texto que se escribe en las terminales del espacio.
//!
//! Las teclas especiales y los atajos (Enter, flechas, Ctrl+C, Alt+letra…) los
//! atiende `console::key_bytes`. El **texto** (letras, símbolos, el espacio,
//! AltGr y las tildes) lo entrega el sistema por aquí, como a cualquier campo
//! de texto: así Windows resuelve solo lo que depende del teclado.
//!
//! Con una tecla muerta, como la tilde `´` del español, Windows manda primero
//! la marca (`replace_and_mark_text_in_range`) y, con la tecla siguiente, el
//! carácter ya compuesto (`replace_text_in_range`: `á`). Mientras hay un
//! texto marcado, GPUI no le pasa esa tecla a la ventana sino al sistema para
//! que componga: por eso `marked_text_range` tiene que decir que sí hay uno.
//! Si se escribiera el texto desde `key_down`, la marca saldría sola y la
//! vocal no se juntaría con ella (`´a`).
//!
//! No se dibuja el texto marcado: una terminal no tiene dónde ponerlo.

use std::ops::Range;

use gpui::{
    canvas, point, px, size, App, Bounds, Context, EntityInputHandler, FocusHandle, InputHandler,
    IntoElement, Pixels, Point, Styled, UTF16Selection, WeakEntity, Window,
};

use super::SpaceView;

impl SpaceView {
    /// Escribe texto en la consola enfocada.
    fn type_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        // Con el selector de carpetas abierto, lo que se escribe lo filtra.
        if let Some(picker) = &mut self.picker {
            picker.type_text(text);
            return;
        }
        if let Some(card) = self.focused.and_then(|id| self.card(id)) {
            // Escribir vuelve al final si se estaba mirando el historial.
            card.console.scroll(i32::MIN / 2);
            card.console.write(text.as_bytes().to_vec());
        }
    }
}

impl EntityInputHandler for SpaceView {
    fn text_for_range(
        &mut self,
        _: Range<usize>,
        _: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        None
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: 0..0,
            reversed: false,
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked.as_ref().map(|text| 0..text.encode_utf16().count())
    }

    fn unmark_text(&mut self, _: &mut Window, _: &mut Context<Self>) {
        self.marked = None;
    }

    fn replace_text_in_range(
        &mut self,
        _: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.marked = None;
        self.type_text(text);
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        _: Option<Range<usize>>,
        text: &str,
        _: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.marked = (!text.is_empty()).then(|| text.to_string());
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        _: Range<usize>,
        bounds: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        // Donde Windows pone la ventanita de un IME: junto al principio de la
        // terminal, que es lo más cerca que se sabe sin medir el cursor.
        Some(Bounds::new(
            point(bounds.origin.x + px(8.), bounds.origin.y + px(8.)),
            size(px(8.), px(18.)),
        ))
    }

    fn character_index_for_point(
        &mut self,
        _: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        None
    }
}

/// Quien recibe el texto de la ventana. Es lo que `ElementInputHandler` hace,
/// pero sin sostener la vista: el sistema guarda este objeto dentro de la
/// ventana nativa, que se libera con retraso al cerrarla, y una referencia
/// fuerte mantendría viva la vista cerrada. Sin soltarse la vista no se
/// guardan las consolas (`persist::stash`) y los agentes se perderían.
struct Receiver {
    view: WeakEntity<SpaceView>,
    bounds: Bounds<Pixels>,
}

impl InputHandler for Receiver {
    fn selected_text_range(
        &mut self,
        ignore_disabled_input: bool,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<UTF16Selection> {
        self.view
            .update(cx, |view, cx| view.selected_text_range(ignore_disabled_input, window, cx))
            .ok()
            .flatten()
    }

    fn marked_text_range(&mut self, window: &mut Window, cx: &mut App) -> Option<Range<usize>> {
        self.view
            .update(cx, |view, cx| view.marked_text_range(window, cx))
            .ok()
            .flatten()
    }

    fn text_for_range(
        &mut self,
        range: Range<usize>,
        adjusted: &mut Option<Range<usize>>,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<String> {
        self.view
            .update(cx, |view, cx| view.text_for_range(range, adjusted, window, cx))
            .ok()
            .flatten()
    }

    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        window: &mut Window,
        cx: &mut App,
    ) {
        let _ = self
            .view
            .update(cx, |view, cx| view.replace_text_in_range(range, text, window, cx));
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        selected: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut App,
    ) {
        let _ = self.view.update(cx, |view, cx| {
            view.replace_and_mark_text_in_range(range, text, selected, window, cx)
        });
    }

    fn unmark_text(&mut self, window: &mut Window, cx: &mut App) {
        let _ = self.view.update(cx, |view, cx| view.unmark_text(window, cx));
    }

    fn bounds_for_range(
        &mut self,
        range: Range<usize>,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<Bounds<Pixels>> {
        let bounds = self.bounds;
        self.view
            .update(cx, |view, cx| view.bounds_for_range(range, bounds, window, cx))
            .ok()
            .flatten()
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<usize> {
        self.view
            .update(cx, |view, cx| view.character_index_for_point(point, window, cx))
            .ok()
            .flatten()
    }
}

/// Un elemento sin dibujo que registra el espacio como quien recibe el texto
/// de la ventana. Tiene que pintarse en cada cuadro, con la vista que se mire.
pub(super) fn layer(view: WeakEntity<SpaceView>, focus: FocusHandle) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |bounds, _, window, cx| {
            let receiver = Receiver {
                view: view.clone(),
                bounds,
            };
            window.handle_input(&focus, receiver, cx);
        },
    )
    .absolute()
    .size_full()
}
