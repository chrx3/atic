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
pub mod files;
pub mod phone;
pub mod proto;
pub mod ticket;

pub use proto::{
    feature, ActivityKind, AgentActivity, AgentCard, AgentQuestion, AgentStatus, CallAction, CallState, ClipItem, ClipKind,
    Decision, FileHeader, FileKind, MediaState, PcCommand, PcState, PermissionAsk, PhoneNotice, QuestionOption,
    RecordingState, MAX_CLIP_BYTES, MAX_FILE_BYTES, MAX_HISTORY_ITEMS, MAX_HISTORY_ITEM_BYTES, MAX_IMAGE_BYTES,
    MAX_THUMB_BYTES,
};

/// Clave nueva para un equipo. Quien la crea la guarda (llavero, archivo
/// privado de la app) y la pasa en cada arranque: es su identidad.
pub fn generate_secret_key() -> [u8; 32] {
    iroh::SecretKey::generate().to_bytes()
}

/// Suma la búsqueda por mDNS: en la misma red, el celular y el PC se encuentran
/// sin internet ni relay. Si la red no deja (sin multicast), se sigue sin ella.
pub(crate) fn add_mdns(endpoint: &iroh::Endpoint) {
    let added = iroh_mdns_address_lookup::MdnsAddressLookup::builder()
        .service_name(MDNS_SERVICE)
        .build(endpoint.id())
        .map_err(|e| anyhow::anyhow!("{e}"))
        .and_then(|mdns| {
            endpoint.address_lookup().map_err(|e| anyhow::anyhow!("{e}"))?.add(mdns);
            Ok(())
        });
    if let Err(e) = added {
        tracing::debug!("sin mDNS: {e:#}");
    }
}

/// El nombre del servicio en la red local; los dos lados tienen que usar el mismo.
const MDNS_SERVICE: &str = "atic";
