//! Lo que el celular controla del PC (`atic_sync::PcCommand` con
//! `feature::CONTROL`), con lo propio de cada sistema en `win32/control.rs` y
//! `macos/control.rs`.

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
}

pub fn run(action: Action) {
    #[cfg(windows)]
    super::win32::control::run(action);
    #[cfg(target_os = "macos")]
    super::macos::control::run(action);
    #[cfg(not(any(windows, target_os = "macos")))]
    let _ = action;
}
