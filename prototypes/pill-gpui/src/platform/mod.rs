//! Lo que depende del sistema operativo, separado del resto de la pill.
//!
//! Hoy solo existe Windows (`win32/`). Para portar a macOS se agrega `macos/`
//! con los mismos módulos y las mismas funciones públicas, y se elige aquí con
//! `cfg`. El resto de la pill los usa como `crate::win`, `crate::paste`, etc.
//!
//! El contrato, módulo por módulo:
//!
//! - `win`: el overlay sobre la ventana de GPUI. Siempre encima sin robar el
//!   foco (`Overlay::attach`, `keep_topmost`, `set_focusable`), dejar pasar
//!   los clics fuera de la pill (`set_passthrough`), ocultarse de las
//!   capturas (`exclude_from_capture`), los monitores (`Screen`, `move_to`,
//!   `cover`) y el cursor y botones globales (`cursor`, `left_button_down`).
//! - `glass`: el vidrio esmerilado detrás de la pill (`Glass`, `Shape`,
//!   `enabled`, `backdrop_available`). En Mac, `NSVisualEffectView`.
//! - `paste`: la ventana destino y pegar en ella (`foreground_target`,
//!   `target_under_cursor`, `force_foreground`, `send_paste_chord`). En Mac,
//!   Cmd+V con CGEvent y permisos de accesibilidad.
//! - `hotkeys`: los atajos globales de `config.json` (`spawn` y `Action`).
//! - `drag`: arrastrar texto, imágenes y archivos hacia otras apps.
//! - `ocr`: leer el texto de una captura.
//! - `privacy`: quién usa el micrófono o la cámara.
//! - `running`: qué apps tienen ventanas abiertas y cerrarlas.
//! - `autostart`: iniciar con el sistema (`sync`). En Mac, un LaunchAgent.
//! - `tray_icon`: el ícono de la bandeja y su menú (`spawn` y `Command`). En
//!   Mac, un `NSStatusItem`.
//!
//! Pendiente: `capture.rs` todavía mezcla la mira (UI) con llamadas a Win32;
//! la parte nativa tiene que pasar aquí antes de portar Capturas.

#[cfg(windows)]
mod win32;
#[cfg(windows)]
pub use win32::{autostart, drag, glass, hotkeys, ocr, paste, privacy, running, tray_icon, win};
