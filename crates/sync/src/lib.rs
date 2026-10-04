//! Canal entre Atic de escritorio y el celular.
//!
//! El escritorio publica la foto de sus agentes; el celular la muestra en la
//! isla y contesta permisos. Va sobre [iroh]: QUIC cifrado de punta a punta,
//! directo en la LAN y por relay cuando no hay camino directo. No hay cuentas:
//! cada lado se identifica por su clave pública y el pareo se hace con un
//! ticket de un solo uso (QR).
//!
//! - [`desktop::Desktop`] escucha, acepta solo equipos pareados y publica.
//! - [`phone::Phone`] se conecta, reconecta solo y manda decisiones.

#[cfg(feature = "uniffi")]
uniffi::setup_scaffolding!();

pub mod desktop;
pub mod phone;
pub mod proto;
pub mod ticket;

pub use proto::{
    ActivityKind, AgentActivity, AgentCard, AgentStatus, ClipItem, ClipKind, Decision, MediaState, PcCommand,
    PcState, PermissionAsk, RecordingState, MAX_CLIP_BYTES, MAX_HISTORY_ITEMS, MAX_HISTORY_ITEM_BYTES,
    MAX_IMAGE_BYTES, MAX_THUMB_BYTES,
};

/// Clave nueva para un equipo. Quien la crea la guarda (llavero, archivo
/// privado de la app) y la pasa en cada arranque: es su identidad.
pub fn generate_secret_key() -> [u8; 32] {
    iroh::SecretKey::generate().to_bytes()
}
