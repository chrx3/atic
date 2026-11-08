//! Un mouse y un teclado para dos computadores, como Universal Control: el
//! cursor sale por un borde de esta pantalla y aparece en el otro equipo, que
//! lo sigue moviendo, y el teclado va detrás. Cada computador conserva su
//! pantalla; no hay monitor virtual ni video.
//!
//! - [`source`]: el equipo con el mouse y el teclado. Escucha la entrada a
//!   bajo nivel; cuando el cursor cruza el borde configurado, se la queda
//!   (el sistema no la ve) y la manda al otro.
//! - [`target`]: el otro equipo. Inyecta lo que llega como si fuera un mouse y
//!   un teclado propios, y avisa cuando el cursor vuelve a cruzar hacia atrás.
//! - [`link`]: la conexión, sobre iroh (QUIC cifrado, directo en la red local),
//!   con su ALPN propio. Los equipos se encuentran por mDNS y se vinculan con
//!   un código de 6 dígitos que muestra el otro.
//! - [`sys`]: lo que depende del sistema (macOS con `CGEvent`, Windows con
//!   ganchos de bajo nivel y `SendInput`).
//! - [`clip`]: el portapapeles compartido (texto, imágenes y archivos).
//! - [`files`]: archivos y carpetas por su propia corriente (copiados o
//!   arrastrados al borde).
//! - [`service`]: todo junto, como lo usa una app: siempre recibe y, si se le
//!   dice, comparte; los dos sentidos a la vez.
//!
//! Las teclas viajan como códigos HID ([`keys`]): no dependen de la
//! distribución de ningún lado, y cada equipo las lee con la suya.

pub mod clip;
pub mod files;
pub mod keys;
pub mod layout;
pub mod link;
pub mod proto;
pub mod service;
pub mod source;
pub mod sys;
pub mod target;

pub use layout::{Rect, Screens, Side};
pub use iroh::EndpointId;
pub use link::Peer;
pub use proto::{Button, Msg, Os};
pub use service::{Event, Service, Share};
