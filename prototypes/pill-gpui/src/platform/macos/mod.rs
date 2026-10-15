//! La implementación de macOS de `platform` (ver `platform/mod.rs`).
//!
//! Coordenadas: el contrato habla de píxeles físicos con el origen arriba a
//! la izquierda, como Windows. En macOS ese espacio son puntos con el origen
//! arriba a la izquierda de la pantalla principal (el de Quartz y
//! `atic-capture`); `win.rs` convierte desde AppKit, que tiene el origen
//! abajo. Lo que en Windows multiplica por la escala para pasar de lógicos a
//! físicos no debe hacerlo en macOS: ahí los dos son puntos.
//!
//! `hotkeys`, `privacy` y `running` son los mismos archivos de Windows: ya
//! traen su versión vacía para los demás sistemas.

pub mod autostart;
pub mod drag;
pub mod glass;
#[path = "../win32/hotkeys.rs"]
pub mod hotkeys;
pub mod ocr;
pub mod paste;
#[path = "../win32/privacy.rs"]
pub mod privacy;
#[path = "../win32/running.rs"]
pub mod running;
pub mod screen_record;
pub mod tray_icon;
pub mod win;
pub mod window_list;
