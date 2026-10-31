//! Lo nativo del flip en macOS. La ventana es un `CGWindowID` de Quartz y su
//! marco va en puntos, como lo que captura `atic-capture` en Mac.
//!
//! Esconder la ventana de otra app pide el permiso de Accesibilidad; aquí se
//! esconde la app entera (`NSRunningApplication::hide`) y se vuelve a mostrar
//! al terminar, que no pide nada.

use objc2::rc::Retained;
use objc2_app_kit::NSRunningApplication;

use crate::geometry::Rect;
use crate::platform::window_list::{self, WindowInfo};

pub(super) struct Hidden {
    app: Retained<NSRunningApplication>,
}

/// Una ventana normal de otra app, de tamaño real.
fn flippable(window: &WindowInfo) -> bool {
    window.layer == 0
        && window.pid != std::process::id() as i32
        && window.width >= 120.0
        && window.height >= 80.0
        && !matches!(window.owner.as_str(), "Atic" | "atic-desktop" | "Window Server" | "Dock")
}

pub(super) fn rect_of(id: isize) -> Option<Rect> {
    let window = window_list::by_id(id as u32)?;
    Some(Rect::new(window.x as f32, window.y as f32, window.width as f32, window.height as f32))
}

/// En macOS el destino de pegar es una app (su pid), no una ventana: el flip
/// usa siempre la ventana más al frente.
pub(super) fn is_flippable(_target: isize) -> bool {
    false
}

pub(super) fn frontmost_real_window() -> Option<isize> {
    window_list::on_screen()
        .into_iter()
        .find(flippable)
        .map(|window| window.id as isize)
}

pub(super) fn title_of(id: isize) -> String {
    window_list::by_id(id as u32)
        .map(|window| if window.title.is_empty() { window.owner } else { window.title })
        .unwrap_or_default()
}

pub(super) fn hide(id: isize) -> Option<Hidden> {
    let window = window_list::by_id(id as u32)?;
    let app = NSRunningApplication::runningApplicationWithProcessIdentifier(window.pid)?;
    app.hide();
    Some(Hidden { app })
}

pub(super) fn restore(hidden: &Hidden) {
    hidden.app.unhide();
}

/// Las ventanas de macOS no cambian de esquinas al maximizarse.
pub(super) fn is_maximized(_id: isize) -> bool {
    false
}
