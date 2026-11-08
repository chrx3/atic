//! Lo que depende del sistema, con el mismo contrato en cada uno:
//!
//! - Para recibir: [`screens`], [`cursor`], [`move_to`], [`button`],
//!   [`wheel`] y [`key`] inyectan como si fueran un mouse y un teclado propios.
//! - Para mandar: [`start_capture`] instala el gancho que le pasa cada evento
//!   a [`crate::source::Grab`]; [`hold_cursor`] deja quieto el cursor mientras
//!   está en el otro y [`release_cursor`] lo devuelve.
//!
//! En los tests todo esto no hace nada: no mueve el cursor de quien los corre.

#[cfg(all(target_os = "macos", not(test)))]
mod macos;
#[cfg(all(target_os = "macos", not(test)))]
pub use macos::*;

#[cfg(all(windows, not(test)))]
mod windows;
#[cfg(all(windows, not(test)))]
pub use self::windows::*;

#[cfg(any(test, not(any(target_os = "macos", windows))))]
mod none {
    use std::sync::Arc;

    use crate::layout::Screens;
    use crate::proto::Button;
    use crate::source::Grab;

    pub const PARK_INSET: f64 = 0.0;

    pub fn prepare() {}
    pub fn screens() -> Screens {
        Screens::default()
    }
    pub fn cursor() -> (f64, f64) {
        (0.0, 0.0)
    }
    pub fn move_to(_x: f64, _y: f64, _dx: f64, _dy: f64) {}
    pub fn button(_button: Button, _down: bool) {}
    pub fn wheel(_dx: f32, _dy: f32) {}
    pub fn key(_hid: u16, _down: bool, _repeat: bool) {}
    pub fn start_capture(_grab: Arc<Grab>) -> anyhow::Result<()> {
        anyhow::bail!("este sistema no puede compartir su mouse y teclado")
    }
    pub fn hold_cursor(_x: f64, _y: f64) {}
    pub fn release_cursor(_x: f64, _y: f64) {}
}
#[cfg(any(test, not(any(target_os = "macos", windows))))]
pub use none::*;
