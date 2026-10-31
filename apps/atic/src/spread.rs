//! macOS: la mira y el cuentagotas en todas las pantallas.
//!
//! Con «Las pantallas tienen espacios separados» (lo normal) una ventana se ve
//! en una sola pantalla, así que el overlay no puede estirarse como en
//! Windows. Cada otra pantalla recibe una ventana espejo que dibuja la misma
//! vista corrida: la sesión es una sola y sus coordenadas son las del overlay,
//! aunque caigan fuera de él.

use gpui::{
    div, point, prelude::*, px, size, AnyElement, AnyWindowHandle, App, Bounds, Context, Entity,
    Focusable, Subscription, Window, WindowBackgroundAppearance, WindowBounds, WindowKind,
    WindowOptions,
};

/// Una vista que se puede dibujar corrida en otra ventana.
pub trait Spread: Focusable + Sized + 'static {
    /// `shift`: dónde está la esquina de la ventana espejo respecto de la del
    /// overlay, en lógico. Lo dibujado se corre `-shift` y los eventos de esa
    /// ventana se corren `+shift` para quedar en coordenadas del overlay.
    fn render_at(&mut self, shift: (f32, f32), window: &mut Window, cx: &mut Context<Self>) -> AnyElement;
}

struct Mirror<V> {
    view: Entity<V>,
    shift: (f32, f32),
    _changes: Subscription,
}

impl<V: Spread> Render for Mirror<V> {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let shift = self.shift;
        let focus = self.view.read(cx).focus_handle(cx);
        let content = self.view.update(cx, |view, cx| view.render_at(shift, window, cx));
        // Si el clic deja a esta ventana con el teclado, que Esc y Enter sigan
        // llegando a la vista.
        div()
            .size_full()
            .on_any_mouse_down(move |_, window, _| window.focus(&focus))
            .child(content)
    }
}

/// Las ventanas espejo abiertas; `close` las cierra.
#[derive(Default)]
pub struct Mirrors(Vec<AnyWindowHandle>);

impl Mirrors {
    /// Una ventana por cada pantalla que no es la del overlay. `origin`: la
    /// esquina del contenido del overlay en pantalla (puntos).
    pub fn open<V: Spread>(view: &Entity<V>, origin: (i32, i32), cx: &mut App) -> Self {
        let mut handles = Vec::new();
        for monitor in atic_capture::monitors::enumerate() {
            let b = monitor.bounds;
            let inside = origin.0 >= b.x && origin.0 < b.right() && origin.1 >= b.y && origin.1 < b.bottom();
            if inside || b.is_empty() {
                continue;
            }
            let shift = ((b.x - origin.0) as f32, (b.y - origin.1) as f32);
            let target = crate::win::RECT {
                left: b.x,
                top: b.y,
                right: b.right(),
                bottom: b.bottom(),
            };
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    point(px(0.), px(0.)),
                    size(px(b.width as f32), px(b.height as f32)),
                ))),
                titlebar: None,
                focus: false,
                show: true,
                kind: WindowKind::PopUp,
                is_movable: false,
                is_resizable: false,
                is_minimizable: false,
                window_background: WindowBackgroundAppearance::Transparent,
                ..Default::default()
            };
            let view = view.clone();
            let opened = cx.open_window(options, |window, cx| {
                #[cfg(target_os = "macos")]
                crate::win::prepare_mirror(window, target);
                #[cfg(not(target_os = "macos"))]
                let _ = (window, target);
                cx.new(|cx| Mirror {
                    _changes: cx.observe(&view, |_, _, cx| cx.notify()),
                    view,
                    shift,
                })
            });
            match opened {
                Ok(handle) => {
                    println!("espejo: pantalla {:?} corrida {shift:?}", (b.x, b.y, b.width, b.height));
                    handles.push(handle.into());
                }
                Err(error) => eprintln!("no se pudo abrir la ventana espejo: {error}"),
            }
        }
        Self(handles)
    }

    pub fn is_open(&self) -> bool {
        !self.0.is_empty()
    }

    pub fn close(&mut self, cx: &mut App) {
        for handle in self.0.drain(..) {
            let _ = handle.update(cx, |_, window, _| window.remove_window());
        }
    }
}
