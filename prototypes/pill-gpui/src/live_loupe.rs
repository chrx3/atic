//! La lupa en vivo de la mira y del cuentagotas lee la pantalla fuera del
//! hilo principal.
//!
//! En macOS cada lectura de 13×13 cuesta ~10 ms (con picos de 50 o más):
//! hecha en `render`, cada cuadro esperaba la foto y la lupa se quedaba atrás
//! del mouse. Ahora la lupa se dibuja donde está el cursor con la última
//! lectura que haya, y se pide la siguiente. Mientras una corre se guarda solo
//! el último pedido: los intermedios ya no sirven.

use atic_capture::Frame;
use gpui::{AppContext, Context};

type Job = Box<dyn FnOnce() -> Option<Frame> + Send>;

#[derive(Default)]
pub struct LiveLoupe {
    /// La última lectura.
    pub frame: Option<Frame>,
    /// Dónde se pidió esa lectura (cada vista usa sus coordenadas).
    pub at: (i32, i32),
    /// Lo último que se pidió, para no repetir pedidos iguales.
    asked: Option<(i32, i32)>,
    /// Número del último pedido, y el de la lectura que se ve: una lectura
    /// vieja que termina tarde no pisa a una más nueva.
    seq: u64,
    shown: u64,
    busy: bool,
    pending: Option<(u64, (i32, i32), Job)>,
}

impl LiveLoupe {
    /// Una lectura hecha al tiro (al abrir o al hacer clic).
    pub fn set(&mut self, at: (i32, i32), frame: Option<Frame>) {
        self.seq += 1;
        self.shown = self.seq;
        self.asked = Some(at);
        self.at = at;
        self.frame = frame;
    }

    /// Pide una lectura en segundo plano; al llegar se redibuja la vista.
    /// `field` lleva de la vista a su lupa.
    pub fn request<V: 'static>(
        &mut self,
        at: (i32, i32),
        job: impl FnOnce() -> Option<Frame> + Send + 'static,
        field: fn(&mut V) -> &mut LiveLoupe,
        cx: &mut Context<V>,
    ) {
        if self.asked == Some(at) {
            return;
        }
        self.asked = Some(at);
        self.seq += 1;
        if self.busy {
            self.pending = Some((self.seq, at, Box::new(job)));
            return;
        }
        self.busy = true;
        let first: (u64, (i32, i32), Job) = (self.seq, at, Box::new(job));
        cx.spawn(async move |view, cx| {
            let mut next = Some(first);
            while let Some((seq, at, job)) = next.take() {
                let frame = cx.background_spawn(async move { job() }).await;
                next = view
                    .update(cx, |view, cx| {
                        let loupe = field(view);
                        if frame.is_some() && seq > loupe.shown {
                            loupe.shown = seq;
                            loupe.at = at;
                            loupe.frame = frame;
                            cx.notify();
                        }
                        let next = loupe.pending.take();
                        loupe.busy = next.is_some();
                        next
                    })
                    .ok()
                    .flatten();
            }
        })
        .detach();
    }
}
