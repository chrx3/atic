//! Lo que el celular controla del PC (`atic_sync::PcCommand` con
//! `feature::CONTROL` o `feature::REMOTE`, y el panel táctil y el teclado de
//! `atic_sync::InputEvent`), con lo propio de cada sistema en
//! `win32/control.rs` y `macos/control.rs`.

pub use atic_sync::{InputEvent, MouseButton, PcKey};

use std::sync::Mutex;

/// El rectángulo (x, y, ancho, alto, en el espacio del cursor del sistema) de la pantalla
/// que el celular está viendo: `InputEvent::PointerTo` va sobre ella y no sobre la
/// principal. `None`: la principal.
static POINTER_SCREEN: Mutex<Option<(i32, i32, u32, u32)>> = Mutex::new(None);

/// Dice sobre qué pantalla caen los toques del celular (`None` al dejar de compartir).
pub fn set_pointer_screen(bounds: Option<(i32, i32, u32, u32)>) {
    *POINTER_SCREEN.lock().unwrap_or_else(|e| e.into_inner()) = bounds;
}

#[cfg(any(windows, target_os = "macos"))]
pub(super) fn pointer_screen() -> Option<(i32, i32, u32, u32)> {
    *POINTER_SCREEN.lock().unwrap_or_else(|e| e.into_inner())
}

/// Una orden del celular al sistema.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    VolumeUp,
    VolumeDown,
    Mute,
    LockScreen,
    /// La diapositiva siguiente o anterior: las flechas a la app del frente.
    SlideNext,
    SlidePrev,
    /// Suspender el PC.
    Sleep,
}

pub fn run(action: Action) {
    #[cfg(windows)]
    super::win32::control::run(action);
    #[cfg(target_os = "macos")]
    super::macos::control::run(action);
    #[cfg(not(any(windows, target_os = "macos")))]
    let _ = action;
}

/// El puntero o el teclado, como si se usaran en este PC.
pub fn input(event: InputEvent) {
    #[cfg(windows)]
    super::win32::control::input(event);
    #[cfg(target_os = "macos")]
    super::macos::control::input(event);
    #[cfg(not(any(windows, target_os = "macos")))]
    let _ = event;
}
