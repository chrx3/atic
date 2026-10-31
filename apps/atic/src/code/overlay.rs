//! La salida animada de lo que flota sobre el chat (menús, popovers, diálogos
//! y toasts), como el `<Presence>` de la referencia: al cerrarse, la capa se sigue
//! dibujando con lo último que mostró mientras se desvanece y luego se quita.
//!
//! `gpui_m3::Presence` no sirve para las capas `deferred`: GPUI no lleva la
//! opacidad del padre a lo que se dibuja diferido, así que la capa pone el
//! `Presence` (o `Exit::apply`) *dentro* de su propio `deferred`.

use std::cell::RefCell;

use gpui::{div, prelude::*, App, Div, ElementId, IntoElement, Window};
use gpui_m3::Exit;

/// Lo último que mostró una capa, para dibujarla mientras sale.
pub(super) struct Last<T>(RefCell<Option<T>>);

impl<T> Default for Last<T> {
    fn default() -> Self {
        Self(RefCell::new(None))
    }
}

/// Qué dibujar de una capa en este cuadro.
pub(super) struct Shown<T> {
    pub value: T,
    /// 1 = a la vista; baja a 0 mientras sale.
    pub progress: f32,
    /// Ya se cerró y solo falta que se desvanezca.
    pub leaving: bool,
}

impl<T: Clone> Last<T> {
    /// `now` es lo que debe verse (nada si está cerrada). Devuelve lo que hay
    /// que dibujar: eso, o lo último que se vio mientras sale; `None` cuando
    /// no hay nada que dibujar. Llámalo en cada cuadro con el mismo `id`.
    pub fn show(&self, id: impl Into<ElementId>, now: Option<T>, window: &mut Window, cx: &mut App) -> Option<Shown<T>> {
        let open = now.is_some();
        let Some(progress) = gpui_m3::motion::presence(id, open, window, cx) else {
            self.0.borrow_mut().take();
            return None;
        };
        if let Some(value) = now {
            *self.0.borrow_mut() = Some(value.clone());
            return Some(Shown { value, progress, leaving: false });
        }
        let value = self.0.borrow().clone()?;
        Some(Shown { value, progress, leaving: true })
    }
}

impl<T> Shown<T> {
    /// El contenido con su desvanecido; mientras sale ignora el puntero.
    pub fn wrap(&self, exit: Exit, content: impl IntoElement) -> Div {
        wrap(exit, self.progress, self.leaving, content)
    }
}

/// `content` con la salida `exit` a `progress`; si `leaving`, una capa encima se
/// traga el puntero hasta que desaparece.
pub(super) fn wrap(exit: Exit, progress: f32, leaving: bool, content: impl IntoElement) -> Div {
    exit.apply(div(), progress).child(content).when(leaving, |d| {
        d.relative().child(div().id("overlay-leaving").absolute().top_0().left_0().size_full().occlude())
    })
}
