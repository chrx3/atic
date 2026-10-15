//! El ícono de la barra de menús. Pendiente en macOS (irá con un
//! `NSStatusItem` y su menú): por ahora el canal no recibe nada.

use std::sync::mpsc::Receiver;

/// Lo que se pidió desde el ícono (ver `win32/tray_icon.rs`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    OpenAtic,
    Settings,
    Consoles,
    Capture,
    Quit,
}

pub fn spawn() -> Receiver<Command> {
    std::sync::mpsc::channel().1
}

/// Sin globos no hay conversación que abrir.
pub fn take_click_topic() -> Option<String> {
    None
}

/// Sin ícono no hay a quién despertar.
pub fn wake_running() -> bool {
    false
}
