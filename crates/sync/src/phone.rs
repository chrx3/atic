//! El lado del celular: se conecta a sus escritorios, reconecta solo y manda
//! decisiones.
//!
//! Un celular puede estar pareado con varios PCs a la vez: una sesión por PC,
//! todas sobre el mismo endpoint. El celular es el centro: lo que llega de un
//! PC, quien hospeda lo reparte a los otros (ver `SyncLink` en Android).

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};

use anyhow::{anyhow, bail, Context, Result};
use iroh::{endpoint::presets, Endpoint, EndpointAddr, SecretKey};
use serde::{Deserialize, Serialize};
use tokio::{
    sync::{mpsc, Notify},
    task::JoinHandle,
};

use crate::{
    proto::{
        read_frame, write_frame, AgentCard, ClipItem, Decision, PcCommand, PcState, ToDesktop, ToPhone, ALPN,
        MAX_CLIP_BYTES,
    },
    ticket::PairingTicket,
};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const BACKOFF_MAX: Duration = Duration::from_secs(30);

pub struct PhoneConfig {
    pub secret_key: [u8; 32],
    /// Cómo aparece en el escritorio ("Galaxy S24 Ultra").
    pub device_name: String,
}

/// El escritorio con el que este celular está pareado. Quien hospeda lo guarda
/// tal cual (ver [`PairedDesktop::to_json`]) y lo pasa en cada arranque.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PairedDesktop {
    pub addr: EndpointAddr,
    pub name: String,
}

impl PairedDesktop {
    /// La clave pública del PC: lo identifica entre los pareados.
    pub fn id(&self) -> String {
        self.addr.id.to_string()
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("siempre serializa")
    }

    pub fn from_json(json: &str) -> Result<Self> {
        serde_json::from_str(json).context("datos del PC dañados")
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum LinkStatus {
    Connecting,
    /// El ticket era bueno; falta que alguien acepte en el PC.
    AwaitingApproval,
    Connected { desktop_name: String },
    /// Se reintenta solo, salvo que el escritorio lo haya rechazado.
    Offline { reason: String },
}

#[derive(Debug, Clone, PartialEq)]
pub enum PhoneEvent {
    Status(LinkStatus),
    Agents(Vec<AgentCard>),
    /// El ticket funcionó: hay que guardar este escritorio.
    Paired(PairedDesktop),
    /// El escritorio no acepta a este celular. No se reintenta.
    Rejected(String),
    /// Algo que se copió en el PC.
    Clip(ClipItem),
    /// Lo que suena y si graba.
    Pc(PcState),
    /// Historial del portapapeles del PC.
    ClipHistory(Vec<ClipItem>),
    /// La imagen entera que se pidió con [`Phone::fetch_image`]. Sin `data`: ya no existe.
    Image { id: String, mime: String, data: Option<Vec<u8>> },
    /// La pill del PC pide controlar la música del celular.
    MediaCommand(PcCommand),
    /// El PC quiere estas imágenes del historial del celular.
    ClipNeed(Vec<String>),
    /// Se borraron en el PC.
    ClipDeleted(Vec<String>),
}

pub enum Target {
    Ticket(PairingTicket),
    Known(PairedDesktop),
}

impl Target {
    pub fn from_ticket(text: &str) -> Result<Self> {
        Ok(Target::Ticket(PairingTicket::decode(text)?))
    }

    /// El PC al que apunta, el mismo id que [`PairedDesktop::id`].
    pub fn desktop_id(&self) -> String {
        self.addr().id.to_string()
    }

    fn addr(&self) -> EndpointAddr {
        match self {
            Target::Ticket(t) => t.addr.clone(),
            Target::Known(d) => d.addr.clone(),
        }
    }
}

type Outbox = Arc<Mutex<Option<mpsc::UnboundedSender<ToDesktop>>>>;

/// La conexión con un PC: su tarea (que reconecta sola) y la cola de salida,
/// que solo existe mientras hay sesión abierta.
struct Session {
    outbox: Outbox,
    task: JoinHandle<()>,
    /// Corta la espera entre intentos: la red cambió y vale la pena probar ya.
    wake: Arc<Notify>,
}

pub struct Phone {
    endpoint: Endpoint,
    device_name: String,
    /// Por id del PC ([`PairedDesktop::id`]).
    sessions: Mutex<HashMap<String, Session>>,
}

impl Phone {
    pub async fn start(config: PhoneConfig) -> Result<Self> {
        let endpoint = Endpoint::builder(presets::N0)
            .secret_key(SecretKey::from_bytes(&config.secret_key))
            .bind()
            .await
            .map_err(|e| anyhow!("no se pudo abrir el canal: {e:#}"))?;
        Ok(Self {
            endpoint,
            device_name: config.device_name,
            sessions: Mutex::new(HashMap::new()),
        })
    }

    /// Se conecta a un PC y queda reconectando hasta [`Phone::disconnect`] o un
    /// rechazo. Reemplaza la conexión anterior con ese mismo PC; las de los
    /// otros siguen. Los eventos de este PC llegan por `events`. Devuelve su id.
    pub fn connect(&self, target: Target, events: mpsc::UnboundedSender<PhoneEvent>) -> String {
        let id = target.desktop_id();
        self.disconnect(&id);
        let outbox: Outbox = Arc::new(Mutex::new(None));
        let wake = Arc::new(Notify::new());
        let task = tokio::spawn(supervise(
            self.endpoint.clone(),
            self.device_name.clone(),
            target,
            events,
            outbox.clone(),
            wake.clone(),
        ));
        self.sessions.lock().unwrap().insert(id.clone(), Session { outbox, task, wake });
        id
    }

    /// La red cambió (wifi ↔ datos, se recuperó la señal). En Android iroh no se
    /// entera solo: hay que avisarle para que busque caminos nuevos. Además, los
    /// PCs sin sesión reintentan ya, sin esperar el próximo intento.
    pub fn network_change(&self) {
        let endpoint = self.endpoint.clone();
        tokio::spawn(async move { endpoint.network_change().await });
        // Solo los que esperan: a uno conectado le quedaría el aviso guardado y
        // reintentaría de inmediato la próxima vez que se corte.
        for session in self.sessions.lock().unwrap().values() {
            if session.outbox.lock().unwrap().is_none() {
                session.wake.notify_one();
            }
        }
    }

    /// Los PCs con sesión abierta ahora.
    pub fn connected(&self) -> Vec<String> {
        self.sessions
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, s)| s.outbox.lock().unwrap().is_some())
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// `false` si ese PC no tiene sesión: la decisión no salió.
    pub fn decide(&self, desktop: &str, agent_id: String, permission_id: String, decision: Decision) -> bool {
        self.send_to(desktop, ToDesktop::Decide { agent_id, permission_id, decision })
    }

    /// Contesta una pregunta del agente. `false` si ese PC no tiene sesión.
    pub fn answer(&self, desktop: &str, agent_id: String, permission_id: String, answers: Vec<String>) -> bool {
        self.send_to(desktop, ToDesktop::Answer { agent_id, permission_id, answers })
    }

    /// Ítems del historial guardado en el celular y lo borrado, para que ese PC
    /// sume. Puede ser todo (al conectarse) o solo lo nuevo que trajo otro PC.
    pub fn sync_clips(&self, desktop: &str, items: Vec<ClipItem>, deleted: Vec<String>) -> bool {
        self.send_to(desktop, ToDesktop::ClipSync { items, deleted })
    }

    /// Una imagen del historial que ese PC pidió.
    pub fn send_clip_image(&self, desktop: &str, id: String, mime: String, data: Vec<u8>) -> bool {
        if data.is_empty() || data.len() > crate::proto::MAX_IMAGE_BYTES {
            return false;
        }
        self.send_to(desktop, ToDesktop::ClipImage { id, mime, data })
    }

    /// El usuario borró ítems en el celular (o llegó un borrado de un PC):
    /// a todos los PCs conectados. `false` si no salió a ninguno.
    pub fn delete_clips(&self, ids: Vec<String>) -> bool {
        !ids.is_empty() && self.broadcast(ToDesktop::ClipDelete { ids }) > 0
    }

    /// Borrados para un PC en particular (los que trajo otro PC).
    pub fn delete_clips_on(&self, desktop: &str, ids: Vec<String>) -> bool {
        !ids.is_empty() && self.send_to(desktop, ToDesktop::ClipDelete { ids })
    }

    /// Lo que suena en el celular, para la pill de todos los PCs. `None`: nada.
    pub fn publish_media(&self, media: Option<crate::proto::MediaState>, art: Option<Vec<u8>>) -> bool {
        let art = art.filter(|a| a.len() <= crate::proto::MAX_THUMB_BYTES);
        self.broadcast(ToDesktop::PhoneMedia { media, art }) > 0
    }

    /// Pide a un PC la imagen entera de un ítem de su historial.
    pub fn fetch_image(&self, desktop: &str, id: String) -> bool {
        self.send_to(desktop, ToDesktop::FetchImage { id })
    }

    /// Manda una imagen al portapapeles de todos los PCs conectados.
    pub fn send_image(&self, mime: String, data: Vec<u8>) -> bool {
        if data.is_empty() || data.len() > crate::proto::MAX_IMAGE_BYTES {
            return false;
        }
        self.broadcast(ToDesktop::Image { mime, data }) > 0
    }

    /// Medios o detener la grabación de un PC. `false` si no tiene sesión.
    pub fn command(&self, desktop: &str, command: PcCommand) -> bool {
        self.send_to(desktop, ToDesktop::Command { command })
    }

    /// Manda texto al portapapeles de todos los PCs conectados. `false` si no
    /// hay ninguno o es demasiado largo.
    pub fn send_clip(&self, text: String, copied_at_ms: i64) -> bool {
        if text.is_empty() || text.len() > MAX_CLIP_BYTES {
            return false;
        }
        let item = ClipItem {
            id: crate::ticket::new_token(),
            text,
            source_name: self.device_name.clone(),
            copied_at_ms,
            pinned: false,
            kind: Default::default(),
            thumb: None,
            width: 0,
            height: 0,
        };
        self.broadcast(ToDesktop::Clip { item }) > 0
    }

    fn send_to(&self, desktop: &str, msg: ToDesktop) -> bool {
        let sessions = self.sessions.lock().unwrap();
        sessions
            .get(desktop)
            .is_some_and(|s| s.outbox.lock().unwrap().as_ref().is_some_and(|tx| tx.send(msg).is_ok()))
    }

    /// A todos los PCs con sesión abierta. Devuelve a cuántos salió.
    fn broadcast(&self, msg: ToDesktop) -> usize {
        let sessions = self.sessions.lock().unwrap();
        sessions
            .values()
            .filter(|s| s.outbox.lock().unwrap().as_ref().is_some_and(|tx| tx.send(msg.clone()).is_ok()))
            .count()
    }

    /// Deja de hablar con un PC (desvincularlo, o reconectar ya).
    pub fn disconnect(&self, desktop: &str) {
        if let Some(session) = self.sessions.lock().unwrap().remove(desktop) {
            session.task.abort();
        }
    }

    pub fn disconnect_all(&self) {
        for (_, session) in self.sessions.lock().unwrap().drain() {
            session.task.abort();
        }
    }

    pub async fn shutdown(self) {
        self.disconnect_all();
        self.endpoint.close().await;
    }
}

enum Ended {
    Closed,
    Rejected(String),
}

async fn supervise(
    endpoint: Endpoint,
    device_name: String,
    mut target: Target,
    events: mpsc::UnboundedSender<PhoneEvent>,
    outbox: Outbox,
    wake: Arc<Notify>,
) {
    let emit = |e: PhoneEvent| {
        let _ = events.send(e);
    };
    let mut backoff = Duration::from_secs(1);
    loop {
        emit(PhoneEvent::Status(LinkStatus::Connecting));
        let mut welcomed = false;
        let ended = session(&endpoint, &device_name, &mut target, &events, &outbox, &mut welcomed).await;
        *outbox.lock().unwrap() = None;
        let reason = match ended {
            Ok(Ended::Rejected(reason)) => {
                emit(PhoneEvent::Rejected(reason.clone()));
                emit(PhoneEvent::Status(LinkStatus::Offline { reason }));
                return;
            }
            Ok(Ended::Closed) => "El PC cerró la conexión.".to_string(),
            Err(e) => format!("{e:#}"),
        };
        if welcomed {
            backoff = Duration::from_secs(1);
        }
        emit(PhoneEvent::Status(LinkStatus::Offline { reason }));
        tokio::select! {
            _ = tokio::time::sleep(backoff) => backoff = (backoff * 2).min(BACKOFF_MAX),
            // Con red nueva, la espera acumulada ya no dice nada.
            _ = wake.notified() => backoff = Duration::from_secs(1),
        }
    }
}

async fn session(
    endpoint: &Endpoint,
    device_name: &str,
    target: &mut Target,
    events: &mpsc::UnboundedSender<PhoneEvent>,
    outbox: &Outbox,
    welcomed: &mut bool,
) -> Result<Ended> {
    let emit = |e: PhoneEvent| {
        let _ = events.send(e);
    };
    let conn = tokio::time::timeout(CONNECT_TIMEOUT, endpoint.connect(target.addr(), ALPN))
        .await
        .context("El PC no respondió.")?
        .map_err(|e| anyhow!("No se pudo llegar al PC: {e:#}"))?;
    let (mut send, mut recv) = conn.open_bi().await?;

    let hello = match target {
        Target::Ticket(t) => ToDesktop::Pair {
            token: t.token.clone(),
            device_name: device_name.to_string(),
        },
        Target::Known(_) => ToDesktop::Hello { device_name: device_name.to_string() },
    };
    write_frame(&mut send, &hello).await?;

    let mut first = read_frame(&mut recv).await?.context("El PC cerró sin responder.")?;
    if first == ToPhone::AwaitingApproval {
        emit(PhoneEvent::Status(LinkStatus::AwaitingApproval));
        first = read_frame(&mut recv).await?.context("El PC cerró sin responder.")?;
    }
    match first {
        ToPhone::Welcome { desktop_name } => {
            if let Target::Ticket(t) = target {
                let desktop = PairedDesktop { addr: t.addr.clone(), name: desktop_name.clone() };
                emit(PhoneEvent::Paired(desktop.clone()));
                // Desde acá, reconectar es saludar: el token ya se gastó.
                *target = Target::Known(desktop);
            }
            *welcomed = true;
            emit(PhoneEvent::Status(LinkStatus::Connected { desktop_name }));
        }
        ToPhone::Rejected { reason } => return Ok(Ended::Rejected(reason)),
        other => bail!("el PC mandó {other:?} antes de saludar"),
    }

    let (tx, mut rx) = mpsc::unbounded_channel();
    *outbox.lock().unwrap() = Some(tx);
    // Lectura en tarea propia: dentro del `select!` se cortaría a medias.
    let (mut incoming, _reader) = crate::proto::spawn_reader::<_, ToPhone>(recv);
    loop {
        tokio::select! {
            msg = incoming.recv() => match msg.unwrap_or(Ok(None))? {
                Some(ToPhone::Agents { agents }) => emit(PhoneEvent::Agents(agents)),
                Some(ToPhone::Clip { item }) => emit(PhoneEvent::Clip(item)),
                Some(ToPhone::Pc { pc }) => emit(PhoneEvent::Pc(pc)),
                Some(ToPhone::ClipHistory { items }) => emit(PhoneEvent::ClipHistory(items)),
                Some(ToPhone::Image { id, mime, data }) => emit(PhoneEvent::Image { id, mime, data }),
                Some(ToPhone::MediaCommand { command }) => emit(PhoneEvent::MediaCommand(command)),
                Some(ToPhone::ClipNeed { ids }) => emit(PhoneEvent::ClipNeed(ids)),
                Some(ToPhone::ClipDeleted { ids }) => emit(PhoneEvent::ClipDeleted(ids)),
                Some(_) => {}
                None => return Ok(Ended::Closed),
            },
            Some(out) = rx.recv() => write_frame(&mut send, &out).await?,
        }
    }
}
