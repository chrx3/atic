//! El lado del escritorio: escucha, acepta solo equipos pareados y publica.

use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use anyhow::{anyhow, bail, Context, Result};
use iroh::{
    endpoint::{presets, Connection, RecvStream, SendStream},
    Endpoint, EndpointId, SecretKey,
};
use tokio::{
    sync::{broadcast, mpsc, oneshot, watch},
    task::JoinHandle,
};
use tracing::debug;

use crate::{
    proto::{
        read_frame, write_frame, AgentCard, ClipItem, Decision, PcCommand, PcState, ToDesktop, ToPhone, ALPN,
        MAX_CLIP_BYTES,
    },
    ticket::{new_token, PairingTicket},
};

/// Cuánto vale un ticket de pareo desde que se muestra el QR.
const PAIRING_TTL: Duration = Duration::from_secs(5 * 60);
/// Un equipo que abre la conexión y no saluda no ocupa una tarea para siempre.
const HELLO_TIMEOUT: Duration = Duration::from_secs(10);
/// Cuánto espera un celular con ticket válido a que alguien acepte en el PC.
const APPROVAL_TIMEOUT: Duration = Duration::from_secs(2 * 60);
/// Código QUIC al cerrar la conexión de un equipo revocado.
const CLOSE_REVOKED: u32 = 1;

pub struct DesktopConfig {
    pub secret_key: [u8; 32],
    /// Cómo se presenta en el celular ("PC de Carlos").
    pub name: String,
    /// Ids de los celulares ya pareados (lo que devolvió [`DesktopEvent::Paired`]).
    pub trusted: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DesktopEvent {
    /// Un equipo trajo un ticket válido. No entra hasta que alguien lo acepte
    /// con [`Desktop::approve_pairing`]: quien saque una foto al QR no se cuela.
    PairRequested { device_id: String, device_name: String },
    /// Un equipo quedó pareado. Quien hospeda guarda `device_id`.
    Paired { device_id: String, device_name: String },
    Connected { device_id: String, device_name: String },
    Disconnected { device_id: String },
    /// Respuesta a un permiso. Quien hospeda la aplica y publica la foto nueva.
    Decide {
        device_id: String,
        agent_id: String,
        permission_id: String,
        decision: Decision,
    },
    /// Respuestas a una pregunta del agente (ver [`ToDesktop::Answer`]).
    Answer {
        device_id: String,
        agent_id: String,
        permission_id: String,
        answers: Vec<String>,
    },
    /// Texto que el usuario mandó desde el celular.
    Clip { device_id: String, item: ClipItem },
    /// Medios o grabación. Quien hospeda lo ejecuta y publica el estado nuevo.
    Command { device_id: String, command: PcCommand },
    /// El celular quiere la imagen entera de un ítem del historial. Quien
    /// hospeda contesta con [`Desktop::send_image`].
    FetchImage { device_id: String, id: String },
    /// Una imagen que mandó el celular para el portapapeles del PC.
    Image { device_id: String, mime: String, data: Vec<u8> },
    /// El historial del celular al conectarse: sumar lo que falte, borrar lo borrado.
    ClipSync { device_id: String, items: Vec<ClipItem>, deleted: Vec<String> },
    /// Una imagen del historial del celular, para sumarla al del PC.
    ClipImage { device_id: String, id: String, mime: String, data: Vec<u8> },
    /// El usuario borró ítems en el celular.
    ClipDelete { device_id: String, ids: Vec<String> },
    /// Lo que suena en el celular. `media` en `None`: nada.
    PhoneMedia {
        device_id: String,
        media: Option<crate::proto::MediaState>,
        art: Option<Vec<u8>>,
    },
}

/// Una imagen para un celular en particular.
struct ImageReply {
    to: EndpointId,
    id: String,
    mime: String,
    data: Option<Vec<u8>>,
}

/// Un texto copiado y de dónde salió, para no devolvérselo a quien lo mandó.
#[derive(Clone)]
struct Outgoing {
    item: ClipItem,
    from: Option<EndpointId>,
}

struct Shared {
    name: String,
    trusted: Mutex<HashSet<EndpointId>>,
    pairing: Mutex<Option<(String, Instant)>>,
    approvals: Mutex<HashMap<EndpointId, oneshot::Sender<bool>>>,
    live: Mutex<HashMap<EndpointId, Connection>>,
    agents: watch::Sender<Vec<AgentCard>>,
    pc: watch::Sender<PcState>,
    history: watch::Sender<Vec<ClipItem>>,
    images: broadcast::Sender<Arc<ImageReply>>,
    media_commands: broadcast::Sender<(EndpointId, PcCommand)>,
    /// Pedidos y avisos para un celular en particular o para todos (`None`).
    notices: broadcast::Sender<(Option<EndpointId>, ToPhone)>,
    clips: broadcast::Sender<Outgoing>,
    events: mpsc::UnboundedSender<DesktopEvent>,
}

impl Shared {
    /// El token vale una sola vez y solo dentro de su plazo.
    fn take_pairing(&self, token: &str) -> bool {
        let mut slot = self.pairing.lock().unwrap();
        match slot.as_ref() {
            Some((t, until)) if t == token && Instant::now() < *until => {
                *slot = None;
                true
            }
            _ => false,
        }
    }

    fn is_trusted(&self, id: &EndpointId) -> bool {
        self.trusted.lock().unwrap().contains(id)
    }

    fn emit(&self, event: DesktopEvent) {
        let _ = self.events.send(event);
    }
}

pub struct Desktop {
    endpoint: Endpoint,
    shared: Arc<Shared>,
    accept_task: JoinHandle<()>,
}

impl Desktop {
    pub async fn start(config: DesktopConfig) -> Result<(Self, mpsc::UnboundedReceiver<DesktopEvent>)> {
        let trusted = config
            .trusted
            .iter()
            .map(|id| id.parse::<EndpointId>().with_context(|| format!("id de equipo inválido: {id}")))
            .collect::<Result<HashSet<_>>>()?;
        let endpoint = Endpoint::builder(presets::N0)
            .secret_key(SecretKey::from_bytes(&config.secret_key))
            .alpns(vec![ALPN.to_vec()])
            .bind()
            .await
            .map_err(|e| anyhow!("no se pudo abrir el canal: {e:#}"))?;

        let (events, rx) = mpsc::unbounded_channel();
        let shared = Arc::new(Shared {
            name: config.name,
            trusted: Mutex::new(trusted),
            pairing: Mutex::new(None),
            approvals: Mutex::new(HashMap::new()),
            live: Mutex::new(HashMap::new()),
            agents: watch::Sender::new(Vec::new()),
            pc: watch::Sender::new(PcState::default()),
            history: watch::Sender::new(Vec::new()),
            images: broadcast::Sender::new(4),
            media_commands: broadcast::Sender::new(8),
            notices: broadcast::Sender::new(16),
            clips: broadcast::Sender::new(16),
            events,
        });
        let accept_task = tokio::spawn(accept_loop(endpoint.clone(), shared.clone()));
        Ok((Self { endpoint, shared, accept_task }, rx))
    }

    pub fn id(&self) -> String {
        self.endpoint.id().to_string()
    }

    /// Abre una ventana de pareo nueva (la anterior deja de valer) y devuelve
    /// el texto del QR. Espera unos segundos a tener relay para que el ticket
    /// sirva también fuera de la LAN.
    pub async fn pairing_ticket(&self) -> String {
        let _ = tokio::time::timeout(Duration::from_secs(5), self.endpoint.online()).await;
        let token = new_token();
        *self.shared.pairing.lock().unwrap() = Some((token.clone(), Instant::now() + PAIRING_TTL));
        PairingTicket {
            addr: self.endpoint.addr(),
            token,
            desktop_name: self.shared.name.clone(),
        }
        .encode()
    }

    /// Cierra la ventana de pareo sin esperar a que venza.
    pub fn cancel_pairing(&self) {
        *self.shared.pairing.lock().unwrap() = None;
    }

    /// Acepta o rechaza un [`DesktopEvent::PairRequested`]. `false` si ese
    /// pedido ya no existe (venció o el celular se fue).
    pub fn approve_pairing(&self, device_id: &str, accept: bool) -> bool {
        let Ok(id) = device_id.parse::<EndpointId>() else {
            return false;
        };
        match self.shared.approvals.lock().unwrap().remove(&id) {
            Some(tx) => tx.send(accept).is_ok(),
            None => false,
        }
    }

    /// Foto completa de los agentes. Les llega a todos los celulares conectados.
    pub fn publish(&self, agents: Vec<AgentCard>) {
        self.shared.agents.send_replace(agents);
    }

    /// Medios y grabación. Solo sale si cambió.
    pub fn publish_pc(&self, pc: PcState) {
        self.shared.pc.send_if_modified(|current| {
            if *current == pc {
                return false;
            }
            *current = pc;
            true
        });
    }

    /// Contesta un [`DesktopEvent::FetchImage`]. `data` en `None`: ya no existe.
    pub fn send_image(&self, device_id: &str, id: String, mime: String, data: Option<Vec<u8>>) -> bool {
        let Ok(to) = device_id.parse::<EndpointId>() else {
            return false;
        };
        let data = data.filter(|d| d.len() <= crate::proto::MAX_IMAGE_BYTES);
        self.shared.images.send(Arc::new(ImageReply { to, id, mime, data })).is_ok()
    }

    /// Pide a un celular las imágenes de su historial que el PC no tiene.
    pub fn request_clip_images(&self, device_id: &str, ids: Vec<String>) -> bool {
        let Ok(to) = device_id.parse::<EndpointId>() else {
            return false;
        };
        !ids.is_empty() && self.shared.notices.send((Some(to), ToPhone::ClipNeed { ids })).is_ok()
    }

    /// Avisa a todos los celulares conectados que se borraron ítems en el PC.
    pub fn clips_deleted(&self, ids: Vec<String>) -> bool {
        !ids.is_empty() && self.shared.notices.send((None, ToPhone::ClipDeleted { ids })).is_ok()
    }

    /// Controla la música de un celular (play/pausa, siguiente, anterior).
    pub fn phone_media_command(&self, device_id: &str, command: PcCommand) -> bool {
        let Ok(to) = device_id.parse::<EndpointId>() else {
            return false;
        };
        self.shared.media_commands.send((to, command)).is_ok()
    }

    /// El historial del portapapeles para el celular. Solo sale si cambió.
    /// Lo que pasa del tope se omite (no se manda cortado).
    pub fn publish_clip_history(&self, items: Vec<ClipItem>) {
        use crate::proto::ClipKind;
        let items: Vec<ClipItem> = items
            .into_iter()
            .filter(|i| match i.kind {
                ClipKind::Text => !i.text.is_empty() && i.text.len() <= crate::proto::MAX_HISTORY_ITEM_BYTES,
                ClipKind::Image => i.thumb.as_ref().is_some_and(|t| t.len() <= crate::proto::MAX_THUMB_BYTES),
            })
            .take(crate::proto::MAX_HISTORY_ITEMS)
            .collect();
        self.shared.history.send_if_modified(|current| {
            if *current == items {
                return false;
            }
            *current = items;
            true
        });
    }

    /// Manda un texto copiado en el PC a los celulares conectados. `false` si
    /// es demasiado largo o no hay nadie conectado.
    pub fn send_clip(&self, text: String, copied_at_ms: i64) -> bool {
        if text.is_empty() || text.len() > MAX_CLIP_BYTES {
            return false;
        }
        let item = ClipItem {
            id: new_token(),
            text,
            source_name: self.shared.name.clone(),
            copied_at_ms,
            pinned: false,
            kind: Default::default(),
            thumb: None,
            width: 0,
            height: 0,
        };
        self.shared.clips.send(Outgoing { item, from: None }).is_ok()
    }

    /// Deja de aceptar a un equipo y corta su conexión si está abierta.
    pub fn revoke(&self, device_id: &str) -> Result<()> {
        let id: EndpointId = device_id.parse().context("id de equipo inválido")?;
        self.shared.trusted.lock().unwrap().remove(&id);
        if let Some(conn) = self.shared.live.lock().unwrap().remove(&id) {
            conn.close(CLOSE_REVOKED.into(), b"revoked");
        }
        Ok(())
    }

    pub async fn shutdown(self) {
        self.accept_task.abort();
        self.endpoint.close().await;
    }
}

async fn accept_loop(endpoint: Endpoint, shared: Arc<Shared>) {
    while let Some(incoming) = endpoint.accept().await {
        let shared = shared.clone();
        tokio::spawn(async move {
            let conn = match incoming.accept() {
                Ok(accepting) => match accepting.await {
                    Ok(conn) => conn,
                    Err(e) => return debug!("conexión fallida: {e:#}"),
                },
                Err(e) => return debug!("conexión rechazada: {e:#}"),
            };
            let device = conn.remote_id();
            if let Err(e) = serve(conn, &shared).await {
                debug!("sesión con {device} terminó: {e:#}");
            }
        });
    }
}

async fn serve(conn: Connection, shared: &Shared) -> Result<()> {
    let device = conn.remote_id();
    let (mut send, mut recv) = tokio::time::timeout(HELLO_TIMEOUT, conn.accept_bi())
        .await
        .context("no saludó a tiempo")??;
    let first: ToDesktop = tokio::time::timeout(HELLO_TIMEOUT, read_frame(&mut recv))
        .await
        .context("no saludó a tiempo")??
        .context("cerró sin saludar")?;

    let device_name = match first {
        ToDesktop::Pair { token, device_name } => {
            if !shared.take_pairing(&token) {
                let reason = "El código de vinculación venció o ya se usó.";
                return reject(&conn, &mut send, reason).await;
            }
            write_frame(&mut send, &ToPhone::AwaitingApproval).await?;
            if !await_approval(shared, device, &device_name).await {
                let reason = "El PC no aceptó la vinculación.";
                return reject(&conn, &mut send, reason).await;
            }
            shared.trusted.lock().unwrap().insert(device);
            shared.emit(DesktopEvent::Paired {
                device_id: device.to_string(),
                device_name: device_name.clone(),
            });
            device_name
        }
        ToDesktop::Hello { device_name } => {
            if !shared.is_trusted(&device) {
                let reason = "Este celular no está vinculado con el PC.";
                return reject(&conn, &mut send, reason).await;
            }
            device_name
        }
        other => bail!("primer mensaje inesperado: {other:?}"),
    };

    write_frame(&mut send, &ToPhone::Welcome { desktop_name: shared.name.clone() }).await?;
    // Una conexión nueva del mismo equipo reemplaza a la anterior.
    if let Some(old) = shared.live.lock().unwrap().insert(device, conn.clone()) {
        old.close(0u32.into(), b"replaced");
    }
    shared.emit(DesktopEvent::Connected {
        device_id: device.to_string(),
        device_name,
    });
    let result = session(&mut send, recv, device, shared).await;
    {
        let mut live = shared.live.lock().unwrap();
        if live.get(&device).is_some_and(|c| c.stable_id() == conn.stable_id()) {
            live.remove(&device);
        }
    }
    shared.emit(DesktopEvent::Disconnected { device_id: device.to_string() });
    result
}

/// Pide la aprobación de quien hospeda y espera la respuesta (o el plazo).
async fn await_approval(shared: &Shared, device: EndpointId, device_name: &str) -> bool {
    let (tx, rx) = oneshot::channel();
    shared.approvals.lock().unwrap().insert(device, tx);
    shared.emit(DesktopEvent::PairRequested {
        device_id: device.to_string(),
        device_name: device_name.to_string(),
    });
    let accepted = matches!(tokio::time::timeout(APPROVAL_TIMEOUT, rx).await, Ok(Ok(true)));
    shared.approvals.lock().unwrap().remove(&device);
    accepted
}

/// Avisa el motivo y espera a que el otro lado cierre: si la conexión se
/// suelta enseguida, QUIC la corta antes de entregar el mensaje y el celular
/// lo vería como un corte de red (y reintentaría).
async fn reject(conn: &Connection, send: &mut SendStream, reason: &str) -> Result<()> {
    write_frame(send, &ToPhone::Rejected { reason: reason.to_string() }).await?;
    let _ = send.finish();
    let _ = tokio::time::timeout(Duration::from_secs(3), conn.closed()).await;
    Ok(())
}

async fn session(send: &mut SendStream, recv: RecvStream, device: EndpointId, shared: &Shared) -> Result<()> {
    let (mut incoming, _reader) = crate::proto::spawn_reader::<_, ToDesktop>(recv);
    let mut agents = shared.agents.subscribe();
    let mut clips = shared.clips.subscribe();
    let mut pc = shared.pc.subscribe();
    let mut history = shared.history.subscribe();
    let mut images = shared.images.subscribe();
    let mut media_commands = shared.media_commands.subscribe();
    let mut notices = shared.notices.subscribe();
    let first = agents.borrow_and_update().clone();
    write_frame(send, &ToPhone::Agents { agents: first }).await?;
    let first_pc = pc.borrow_and_update().clone();
    write_frame(send, &ToPhone::Pc { pc: first_pc }).await?;
    let first_history = history.borrow_and_update().clone();
    write_frame(send, &ToPhone::ClipHistory { items: first_history }).await?;
    loop {
        tokio::select! {
            changed = agents.changed() => {
                if changed.is_err() {
                    return Ok(());
                }
                let snapshot = agents.borrow_and_update().clone();
                write_frame(send, &ToPhone::Agents { agents: snapshot }).await?;
            }
            changed = pc.changed() => {
                if changed.is_err() {
                    return Ok(());
                }
                let state = pc.borrow_and_update().clone();
                write_frame(send, &ToPhone::Pc { pc: state }).await?;
            }
            changed = history.changed() => {
                if changed.is_err() {
                    return Ok(());
                }
                let items = history.borrow_and_update().clone();
                write_frame(send, &ToPhone::ClipHistory { items }).await?;
            }
            notice = notices.recv() => match notice {
                Ok((to, msg)) if to.is_none_or(|t| t == device) => write_frame(send, &msg).await?,
                Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => {}
                Err(broadcast::error::RecvError::Closed) => return Ok(()),
            },
            command = media_commands.recv() => match command {
                Ok((to, command)) if to == device => {
                    write_frame(send, &ToPhone::MediaCommand { command }).await?;
                }
                Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => {}
                Err(broadcast::error::RecvError::Closed) => return Ok(()),
            },
            reply = images.recv() => match reply {
                Ok(reply) if reply.to == device => {
                    write_frame(send, &ToPhone::Image {
                        id: reply.id.clone(),
                        mime: reply.mime.clone(),
                        data: reply.data.clone(),
                    })
                    .await?;
                }
                Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => {}
                Err(broadcast::error::RecvError::Closed) => return Ok(()),
            },
            clip = clips.recv() => match clip {
                Ok(Outgoing { item, from }) if from != Some(device) => {
                    write_frame(send, &ToPhone::Clip { item }).await?;
                }
                Ok(_) => {}
                // Si el celular se atrasó, se pierden los copiados viejos: solo importa el último.
                Err(broadcast::error::RecvError::Lagged(_)) => {}
                Err(broadcast::error::RecvError::Closed) => return Ok(()),
            },
            msg = incoming.recv() => match msg.unwrap_or(Ok(None))? {
                None => return Ok(()),
                // Un equipo revocado con la sesión abierta ya no decide ni copia nada.
                Some(_) if !shared.is_trusted(&device) => {}
                Some(ToDesktop::Decide { agent_id, permission_id, decision }) => {
                    shared.emit(DesktopEvent::Decide {
                        device_id: device.to_string(),
                        agent_id,
                        permission_id,
                        decision,
                    });
                }
                Some(ToDesktop::Answer { agent_id, permission_id, answers }) => {
                    shared.emit(DesktopEvent::Answer {
                        device_id: device.to_string(),
                        agent_id,
                        permission_id,
                        answers,
                    });
                }
                Some(ToDesktop::ClipSync { items, deleted }) => {
                    shared.emit(DesktopEvent::ClipSync { device_id: device.to_string(), items, deleted });
                }
                Some(ToDesktop::ClipImage { id, mime, data }) => {
                    if !data.is_empty() && data.len() <= crate::proto::MAX_IMAGE_BYTES {
                        shared.emit(DesktopEvent::ClipImage { device_id: device.to_string(), id, mime, data });
                    }
                }
                Some(ToDesktop::ClipDelete { ids }) => {
                    shared.emit(DesktopEvent::ClipDelete { device_id: device.to_string(), ids });
                }
                Some(ToDesktop::PhoneMedia { media, art }) => {
                    let art = art.filter(|a| a.len() <= crate::proto::MAX_THUMB_BYTES);
                    shared.emit(DesktopEvent::PhoneMedia { device_id: device.to_string(), media, art });
                }
                Some(ToDesktop::FetchImage { id }) => {
                    shared.emit(DesktopEvent::FetchImage { device_id: device.to_string(), id });
                }
                Some(ToDesktop::Image { mime, data }) => {
                    if !data.is_empty() && data.len() <= crate::proto::MAX_IMAGE_BYTES {
                        shared.emit(DesktopEvent::Image { device_id: device.to_string(), mime, data });
                    }
                }
                Some(ToDesktop::Command { command }) => {
                    shared.emit(DesktopEvent::Command { device_id: device.to_string(), command });
                }
                Some(ToDesktop::Clip { item }) => {
                    if item.text.is_empty() || item.text.len() > MAX_CLIP_BYTES {
                        continue;
                    }
                    let _ = shared.clips.send(Outgoing { item: item.clone(), from: Some(device) });
                    shared.emit(DesktopEvent::Clip { device_id: device.to_string(), item });
                }
                Some(other) => debug!("mensaje fuera de lugar de {device}: {other:?}"),
            },
        }
    }
}
