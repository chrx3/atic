//! Motor de capturas de pantalla para Atic (Windows/GDI).
//!
//! Enumera monitores y ventanas, captura a memoria con `BitBlt` /
//! `PrintWindow`, codifica a PNG y limpia archivos por antigüedad. No depende
//! de Tauri ni del frontend, para poder probarse de forma aislada.
//!
//! Los módulos puros (`geometry`, `frame`, `encoding`, `naming`, `retention`)
//! compilan y se prueban en cualquier plataforma. Los módulos que llaman a la
//! API Win32 (`monitors`, `windows`, `engine`) solo existen en Windows.

pub mod encoding;
pub mod error;
pub mod frame;
pub mod geometry;
pub mod naming;
pub mod retention;

pub use error::{Error, Result};
pub use frame::Frame;
pub use geometry::Rect;

#[cfg(windows)]
pub mod engine;
#[cfg(windows)]
pub mod monitors;
#[cfg(windows)]
pub mod windows;

#[cfg(windows)]
pub use monitors::MonitorInfo;
#[cfg(windows)]
pub use windows::WindowCandidate;
