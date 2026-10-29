//! Celular con celular: mensajes de texto y zumbidos entre dos Atic vinculados.
//!
//! Va por el mismo endpoint (y la misma identidad) que [`crate::phone::Phone`],
//! con su propio ALPN. A diferencia del PC, acá los dos lados escuchan y los dos
//! llaman: cada mensaje abre una conexión, espera el [`PeerMsg::Ack`] y cierra.
//! No hay sesión que mantener; si el otro no está, quien manda reintenta después
//! (el que recibe descarta los `id` repetidos, así que reintentar es seguro).
//!
//! El vínculo se hace con un código de un solo uso (QR, `aticp1:`) que muestra
//! uno y escanea el otro. Desde ahí cada uno acepta solo a los que conoce.

use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex},
    time::Duration,
};

use anyhow::{anyhow, bail, Context, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use iroh::{endpoint::Connection, Endpoint, EndpointAddr, EndpointId};
use serde::{Deserialize, Serialize};
use tokio::{sync::mpsc, task::JoinHandle, time::Instant};
use tracing::debug;

use crate::{
    proto::{read_frame_max, write_frame, MAX_GREETING},
    ticket::new_token,
};

/// Versión del protocolo entre celulares: un cambio incompatible cambia el ALPN.
pub const PEER_ALPN: &[u8] = b"atic/peer/1";

const PREFIX: &str = "aticp1:";
/// Lo que vale un código de vínculo antes de vencer.
const PAIRING_TTL: Duration = Duration::from_secs(5 * 60);
/// Para encontrar al otro (directo o por relay) y abrir la conexión.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
/// Para el primer mensaje y para la respuesta.
const REPLY_TIMEOUT: Duration = Duration::from_secs(15);
/// Conexiones entrantes a la vez: un celular manda de a una.
const MAX_CONNECTIONS: usize = 16;
/// Cuántos `id` recordar para descartar reintentos de algo que ya llegó.
const SEEN_MAX: usize = 512;

/// Lo más largo que puede ser un mensaje de texto, en bytes (UTF-8).
pub const MAX_TEXT_BYTES: usize = 4096;
/// Un zumbido: hasta tantos golpes...
pub const MAX_BUZZ_STEPS: usize = 32;
/// ...y que no dure más que esto en total (vibración más pausas).
pub const MAX_BUZZ_MS: u32 = 3000;

/// Un golpe de un zumbido: vibra `on_ms` con fuerza `amplitude` (1–255) y
/// después calla `off_ms`. El que recibe lo pasa tal cual a la vibración.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct BuzzStep {
    pub on_ms: u16,
    pub off_ms: u16,
    pub amplitude: u8,
}

/// Si un zumbido se puede mandar (y recibir): que no esté vacío, no pase de
/// [`MAX_BUZZ_STEPS`] golpes ni de [`MAX_BUZZ_MS`], y que vibre algo.
pub fn check_buzz(steps: &[BuzzStep]) -> Result<()> {
    if steps.is_empty() {
        bail!("el zumbido está vacío");
    }
    if steps.len() > MAX_BUZZ_STEPS {
        bail!("el zumbido tiene más de {MAX_BUZZ_STEPS} golpes");
    }
    let total: u32 = steps.iter().map(|s| u32::from(s.on_ms) + u32::from(s.off_ms)).sum();
    if total > MAX_BUZZ_MS {
        bail!("el zumbido dura más de {MAX_BUZZ_MS} ms");
    }
    if !steps.iter().any(|s| s.on_ms > 0 && s.amplitude > 0) {
        bail!("el zumbido no vibra");
    }
    Ok(())
}

/// Si un texto se puede mandar: que tenga algo y no pase de [`MAX_TEXT_BYTES`].
pub fn check_text(text: &str) -> Result<()> {
    if text.trim().is_empty() {
        bail!("el mensaje está vacío");
    }
    if text.len() > MAX_TEXT_BYTES {
        bail!("el mensaje pasa de {MAX_TEXT_BYTES} bytes");
    }
    Ok(())
}

/// El código de vínculo: lo que muestra el QR de un celular para que otro lo escanee.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PeerTicket {
    pub addr: EndpointAddr,
    pub token: String,
    pub name: String,
}

impl PeerTicket {
    pub fn encode(&self) -> String {
        let json = serde_json::to_vec(self).expect("el código siempre serializa");
        format!("{PREFIX}{}", URL_SAFE_NO_PAD.encode(json))
    }

    pub fn decode(text: &str) -> Result<Self> {
        let Some(body) = text.trim().strip_prefix(PREFIX) else {
            bail!("no es un código de Atic para celulares");
        };
        let json = URL_SAFE_NO_PAD.decode(body).context("código dañado")?;
        serde_json::from_slice(&json).context("código dañado")
    }
}

/// Un celular vinculado. Quien hospeda lo guarda tal cual ([`PairedPeer::to_json`])
/// y lo pasa en cada arranque.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PairedPeer {
    pub addr: EndpointAddr,
    pub name: String,
}

impl PairedPeer {
    /// Su clave pública: lo identifica.
    pub fn id(&self) -> String {
        self.addr.id.to_string()
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("siempre serializa")
    }

    pub fn from_json(json: &str) -> Result<Self> {
        serde_json::from_str(json).context("datos del celular dañados")
    }
}

/// Lo que va por el cable. Cada conexión lleva un mensaje y su respuesta.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
enum PeerMsg {
    /// Vincularse con el código. Lleva la dirección de quien escaneó, para que
    /// el otro lo pueda llamar después sin buscarlo.
    Pair { token: String, name: String, addr: EndpointAddr },
    /// Vínculo aceptado; el nombre de quien mostró el código.
    Paired { name: String },
    Rejected { reason: String },
    Text { id: String, text: String, sent_at_ms: i64 },
    Buzz { id: String, steps: Vec<BuzzStep>, sent_at_ms: i64 },
    /// Llegó (o ya había llegado) el mensaje `id`.
    Ack { id: String },
    /// Quien manda ya no quiere el vínculo.
    Unpair,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PeerEvent {
    /// Se hizo un vínculo nuevo (de cualquiera de los dos lados): guardarlo.
    Paired { peer: PairedPeer },
    Text { peer_id: String, id: String, text: String, sent_at_ms: i64 },
    Buzz { peer_id: String, id: String, steps: Vec<BuzzStep>, sent_at_ms: i64 },
    /// El otro deshizo el vínculo: olvidarlo.
    Unpaired { peer_id: String },
}

struct Shared {
    name: String,
    known: Mutex<HashMap<EndpointId, PairedPeer>>,
    pairing: Mutex<Option<(String, Instant)>>,
    seen: Mutex<VecDeque<(EndpointId, String)>>,
    events: mpsc::UnboundedSender<PeerEvent>,
}

impl Shared {
    fn is_known(&self, id: &EndpointId) -> bool {
        self.known.lock().unwrap().contains_key(id)
    }

    /// Gasta el código si es el vigente.
    fn take_pairing(&self, token: &str) -> bool {
        let mut pairing = self.pairing.lock().unwrap();
        match pairing.as_ref() {
            Some((t, until)) if t == token && Instant::now() < *until => {
                *pairing = None;
                true
            }
            _ => false,
        }
    }

    /// `true` la primera vez que llega este `id` de este celular.
    fn first_time(&self, from: EndpointId, id: &str) -> bool {
        let mut seen = self.seen.lock().unwrap();
        if seen.iter().any(|(f, i)| *f == from && i == id) {
            return false;
        }
        if seen.len() >= SEEN_MAX {
            seen.pop_front();
        }
        seen.push_back((from, id.to_string()));
        true
    }

    fn emit(&self, event: PeerEvent) {
        let _ = self.events.send(event);
    }
}

/// Los celulares vinculados: escucha a los que conoce y les manda mensajes.
pub struct Peers {
    endpoint: Endpoint,
    shared: Arc<Shared>,
    task: JoinHandle<()>,
}

impl Peers {
    /// Empieza a escuchar en `endpoint` (el de [`crate::phone::Phone`]) con los
    /// vinculados de antes. Llamar dentro del runtime de tokio.
    pub fn start(endpoint: Endpoint, name: String, known: Vec<PairedPeer>, events: mpsc::UnboundedSender<PeerEvent>) -> Self {
        endpoint.set_alpns(vec![PEER_ALPN.to_vec()]);
        let shared = Arc::new(Shared {
            name,
            known: Mutex::new(known.into_iter().map(|p| (p.addr.id, p)).collect()),
            pairing: Mutex::new(None),
            seen: Mutex::new(VecDeque::new()),
            events,
        });
        let task = tokio::spawn(accept_loop(endpoint.clone(), shared.clone()));
        Self { endpoint, shared, task }
    }

    /// Suma uno guardado (por ejemplo, restaurado después de arrancar).
    pub fn add(&self, peer: PairedPeer) {
        self.shared.known.lock().unwrap().insert(peer.addr.id, peer);
    }

    /// Los vinculados ahora.
    pub fn known(&self) -> Vec<PairedPeer> {
        self.shared.known.lock().unwrap().values().cloned().collect()
    }

    /// Abre una ventana de vínculo nueva (la anterior deja de valer) y devuelve
    /// el texto del QR. Espera unos segundos a tener relay para que el código
    /// sirva también fuera de la misma red.
    pub async fn ticket(&self) -> String {
        let _ = tokio::time::timeout(Duration::from_secs(5), self.endpoint.online()).await;
        let token = new_token();
        *self.shared.pairing.lock().unwrap() = Some((token.clone(), Instant::now() + PAIRING_TTL));
        PeerTicket { addr: self.endpoint.addr(), token, name: self.shared.name.clone() }.encode()
    }

    /// Cierra la ventana de vínculo sin esperar a que venza.
    pub fn cancel_ticket(&self) {
        *self.shared.pairing.lock().unwrap() = None;
    }

    /// Se vincula con el código que mostró el otro. Si sale bien, también llega
    /// [`PeerEvent::Paired`].
    pub async fn pair(&self, ticket: &str) -> Result<PairedPeer> {
        let ticket = PeerTicket::decode(ticket)?;
        if ticket.addr.id == self.endpoint.id() {
            bail!("es el código de este mismo celular");
        }
        let hello = PeerMsg::Pair { token: ticket.token, name: self.shared.name.clone(), addr: self.endpoint.addr() };
        match self.exchange(ticket.addr.clone(), &hello).await? {
            PeerMsg::Paired { name } => {
                let peer = PairedPeer { addr: ticket.addr, name };
                self.add(peer.clone());
                self.shared.emit(PeerEvent::Paired { peer: peer.clone() });
                Ok(peer)
            }
            PeerMsg::Rejected { reason } => Err(anyhow!(reason)),
            other => bail!("respuesta inesperada: {other:?}"),
        }
    }

    /// Manda un texto y espera a que llegue. `Err` si no se pudo: reintentar después.
    pub async fn send_text(&self, peer_id: &str, id: String, text: String, sent_at_ms: i64) -> Result<()> {
        check_text(&text)?;
        self.deliver(peer_id, PeerMsg::Text { id: id.clone(), text, sent_at_ms }, &id).await
    }

    /// Manda un zumbido y espera a que llegue. `Err` si no se pudo.
    pub async fn send_buzz(&self, peer_id: &str, id: String, steps: Vec<BuzzStep>, sent_at_ms: i64) -> Result<()> {
        check_buzz(&steps)?;
        self.deliver(peer_id, PeerMsg::Buzz { id: id.clone(), steps, sent_at_ms }, &id).await
    }

    /// Olvida a ese celular y, si se puede, le avisa para que haga lo mismo.
    pub async fn unpair(&self, peer_id: &str) {
        let Some(peer) = self.find(peer_id) else { return };
        self.shared.known.lock().unwrap().remove(&peer.addr.id);
        if let Err(e) = self.exchange(peer.addr, &PeerMsg::Unpair).await {
            debug!("no se pudo avisar el desvínculo a {peer_id}: {e:#}");
        }
    }

    fn find(&self, peer_id: &str) -> Option<PairedPeer> {
        self.shared.known.lock().unwrap().values().find(|p| p.id() == peer_id).cloned()
    }

    async fn deliver(&self, peer_id: &str, msg: PeerMsg, id: &str) -> Result<()> {
        let peer = self.find(peer_id).context("ese celular no está vinculado")?;
        match self.exchange(peer.addr, &msg).await? {
            PeerMsg::Ack { id: got } if got == id => Ok(()),
            PeerMsg::Rejected { reason } => Err(anyhow!(reason)),
            other => bail!("respuesta inesperada: {other:?}"),
        }
    }

    /// Una conexión, un mensaje y su respuesta.
    async fn exchange(&self, addr: EndpointAddr, msg: &PeerMsg) -> Result<PeerMsg> {
        let conn = tokio::time::timeout(CONNECT_TIMEOUT, self.endpoint.connect(addr, PEER_ALPN))
            .await
            .context("el otro celular no aparece")?
            .map_err(|e| anyhow!("no se pudo conectar: {e:#}"))?;
        let reply = async {
            let (mut send, mut recv) = conn.open_bi().await?;
            write_frame(&mut send, msg).await?;
            let reply: Option<PeerMsg> = tokio::time::timeout(REPLY_TIMEOUT, read_frame_max(&mut recv, MAX_GREETING))
                .await
                .context("el otro celular no contestó")??;
            reply.context("el otro celular cerró sin contestar")
        }
        .await;
        conn.close(0u32.into(), b"done");
        reply
    }
}

impl Drop for Peers {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn accept_loop(endpoint: Endpoint, shared: Arc<Shared>) {
    let slots = Arc::new(tokio::sync::Semaphore::new(MAX_CONNECTIONS));
    while let Some(incoming) = endpoint.accept().await {
        let Ok(slot) = slots.clone().try_acquire_owned() else {
            debug!("demasiadas conexiones de celulares; se rechaza una");
            incoming.refuse();
            continue;
        };
        let shared = shared.clone();
        tokio::spawn(async move {
            let _slot = slot;
            let conn = match incoming.accept() {
                Ok(accepting) => match accepting.await {
                    Ok(conn) => conn,
                    Err(e) => return debug!("conexión de celular fallida: {e:#}"),
                },
                Err(e) => return debug!("conexión de celular rechazada: {e:#}"),
            };
            let from = conn.remote_id();
            if let Err(e) = serve(&conn, &shared).await {
                debug!("mensaje de {from} falló: {e:#}");
            }
        });
    }
}

async fn serve(conn: &Connection, shared: &Shared) -> Result<()> {
    let from = conn.remote_id();
    let (mut send, mut recv) = tokio::time::timeout(REPLY_TIMEOUT, conn.accept_bi()).await.context("no mandó nada a tiempo")??;
    let msg: PeerMsg = tokio::time::timeout(REPLY_TIMEOUT, read_frame_max(&mut recv, MAX_GREETING))
        .await
        .context("no mandó nada a tiempo")??
        .context("cerró sin mandar nada")?;

    let reply = match msg {
        PeerMsg::Pair { token, name, addr } => {
            // Si se cortó justo después de aceptar, vuelve con el código ya gastado: ya es conocido.
            if shared.is_known(&from) || shared.take_pairing(&token) {
                // La dirección la manda él; vale solo si es la suya.
                let addr = if addr.id == from { addr } else { EndpointAddr::new(from) };
                let peer = PairedPeer { addr, name };
                shared.known.lock().unwrap().insert(from, peer.clone());
                shared.emit(PeerEvent::Paired { peer });
                PeerMsg::Paired { name: shared.name.clone() }
            } else {
                PeerMsg::Rejected { reason: "El código venció o ya se usó.".into() }
            }
        }
        _ if !shared.is_known(&from) => PeerMsg::Rejected { reason: "Este celular no está vinculado.".into() },
        PeerMsg::Text { id, text, sent_at_ms } => match check_text(&text) {
            Err(e) => PeerMsg::Rejected { reason: e.to_string() },
            Ok(()) => {
                if shared.first_time(from, &id) {
                    shared.emit(PeerEvent::Text { peer_id: from.to_string(), id: id.clone(), text, sent_at_ms });
                }
                PeerMsg::Ack { id }
            }
        },
        PeerMsg::Buzz { id, steps, sent_at_ms } => match check_buzz(&steps) {
            Err(e) => PeerMsg::Rejected { reason: e.to_string() },
            Ok(()) => {
                if shared.first_time(from, &id) {
                    shared.emit(PeerEvent::Buzz { peer_id: from.to_string(), id: id.clone(), steps, sent_at_ms });
                }
                PeerMsg::Ack { id }
            }
        },
        PeerMsg::Unpair => {
            shared.known.lock().unwrap().remove(&from);
            shared.emit(PeerEvent::Unpaired { peer_id: from.to_string() });
            PeerMsg::Ack { id: String::new() }
        }
        other => bail!("mensaje inesperado: {other:?}"),
    };
    write_frame(&mut send, &reply).await?;
    send.finish()?;
    // Quien mandó cierra al leer la respuesta; sin esperarlo, la respuesta podía no salir.
    let _ = tokio::time::timeout(REPLY_TIMEOUT, conn.closed()).await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(on: u16, off: u16) -> BuzzStep {
        BuzzStep { on_ms: on, off_ms: off, amplitude: 200 }
    }

    #[test]
    fn un_codigo_va_y_vuelve_igual() {
        let ticket = PeerTicket { addr: EndpointAddr::new(iroh::SecretKey::generate().public()), token: new_token(), name: "Celular".into() };
        let text = ticket.encode();
        assert!(text.starts_with(PREFIX));
        assert_eq!(PeerTicket::decode(&text).unwrap(), ticket);
    }

    #[test]
    fn el_codigo_del_pc_no_sirve_para_celulares() {
        assert!(PeerTicket::decode("atic1:abc").is_err());
        assert!(PeerTicket::decode("aticp1:%%%").is_err());
    }

    #[test]
    fn zumbidos_dentro_de_los_topes() {
        assert!(check_buzz(&[step(100, 50), step(300, 0)]).is_ok());
        assert!(check_buzz(&[]).is_err());
        assert!(check_buzz(&vec![step(10, 10); MAX_BUZZ_STEPS + 1]).is_err());
        assert!(check_buzz(&[step(2000, 1001)]).is_err());
        assert!(check_buzz(&[BuzzStep { on_ms: 100, off_ms: 0, amplitude: 0 }]).is_err());
    }

    #[test]
    fn textos_dentro_de_los_topes() {
        assert!(check_text("hola").is_ok());
        assert!(check_text("   ").is_err());
        assert!(check_text(&"a".repeat(MAX_TEXT_BYTES + 1)).is_err());
    }
}
