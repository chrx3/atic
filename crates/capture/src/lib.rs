//! Motor de capturas de pantalla para Atic.
//!
//! Enumera monitores (Windows y macOS), captura a memoria (GDI en Windows,
//! Core Graphics en macOS), codifica a PNG y limpia archivos por antigüedad.
//! No depende de Tauri ni del frontend, para poder probarse de forma aislada.
//!
//! Los módulos puros (`geometry`, `frame`, `encoding`, `naming`, `retention`)
//! y `monitors` compilan en cualquier plataforma. `windows` (candidatas) y
//! `engine` existen en Windows y macOS.

pub mod encoding;
pub mod error;
pub mod frame;
pub mod geometry;
pub mod monitors;
pub mod naming;
pub mod retention;

pub use error::{Error, Result};
pub use frame::Frame;
pub use geometry::Rect;
pub use monitors::MonitorInfo;

#[cfg(any(windows, target_os = "macos"))]
pub mod engine;
#[cfg(any(windows, target_os = "macos"))]
pub mod windows;

#[cfg(any(windows, target_os = "macos"))]
pub use windows::WindowCandidate;
