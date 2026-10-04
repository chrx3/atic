//! El ticket de pareo: lo que muestra el QR del escritorio.
//!
//! Lleva la dirección del escritorio (id, relay, IPs locales) y un token de un
//! solo uso. Quien lo escanea demuestra que vio la pantalla del escritorio; el
//! token expira y se gasta al primer uso.

use anyhow::{bail, Context, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use iroh::EndpointAddr;
use serde::{Deserialize, Serialize};

const PREFIX: &str = "atic1:";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PairingTicket {
    pub addr: EndpointAddr,
    pub token: String,
    pub desktop_name: String,
}

impl PairingTicket {
    pub fn encode(&self) -> String {
        let json = serde_json::to_vec(self).expect("el ticket siempre serializa");
        format!("{PREFIX}{}", URL_SAFE_NO_PAD.encode(json))
    }

    pub fn decode(text: &str) -> Result<Self> {
        let Some(body) = text.trim().strip_prefix(PREFIX) else {
            bail!("no es un ticket de Atic");
        };
        let json = URL_SAFE_NO_PAD.decode(body).context("ticket dañado")?;
        serde_json::from_slice(&json).context("ticket dañado")
    }
}

/// Token aleatorio de 128 bits, en hex.
pub fn new_token() -> String {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).expect("el sistema siempre da aleatoriedad");
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn un_ticket_va_y_vuelve_igual() {
        let ticket = PairingTicket {
            addr: EndpointAddr::new(iroh::SecretKey::generate().public()),
            token: new_token(),
            desktop_name: "PC de prueba".into(),
        };
        let text = ticket.encode();
        assert!(text.starts_with(PREFIX));
        assert_eq!(PairingTicket::decode(&text).unwrap(), ticket);
    }

    #[test]
    fn rechaza_texto_que_no_es_ticket() {
        assert!(PairingTicket::decode("https://example.com").is_err());
        assert!(PairingTicket::decode("atic1:%%%").is_err());
    }

    #[test]
    fn los_tokens_no_se_repiten() {
        assert_ne!(new_token(), new_token());
        assert_eq!(new_token().len(), 32);
    }
}
