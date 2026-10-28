//! El lado del celular: se conecta a sus escritorios, reconecta solo y manda
//! decisiones.
//!
//! Un celular puede estar pareado con varios PCs a la vez: una sesión por PC,
//! todas sobre el mismo endpoint. El celular es el centro: lo que llega de un
//! PC, quien hospeda lo reparte a los otros (ver `SyncLink` en Android).

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

use anyhow::{anyhow, bail, Context, Result};
use iroh::{
    endpoint::{presets, Connection},
    Endpoint, EndpointAddr, SecretKey,
};
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
    proto::{feature, CallAction, CallState, FileHeader, FileKind, InputEvent, PhoneNotice, MAX_INPUT_TEXT},
    ticket::PairingTicket,
};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const BACKOFF_MAX: Duration = Duration::from_secs(30);
/// Cada cuánto se vuelve a probar con un PC que dijo no reconocer al celular:
/// al principio cada 2 minutos, y cada vez más espaciado hasta media hora (si
/// lo desvincularon a propósito, no tiene sentido insistir seguido).
const REJECTED_RETRY: Duration = Duration::from_secs(120);
const REJECTED_RETRY_MAX: Duration = Duration::from_secs(30 * 60);

pub struct PhoneConfig {
    pub secret_key: [u8; 32],
    /// Cómo aparece en el escritorio ("Galaxy S24 Ultra").
    pub device_name: String,
    /// Dónde quedan los archivos que mandan los PCs. Sin carpeta no se reciben.
    pub inbox: Option<PathBuf>,
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
    /// `features`: lo que sabe hacer ese PC (ver [`feature`]). Un PC viejo, ninguna.
    Connected { desktop_name: String, features: Vec<String> },
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
    /// Contestar una notificación del celular desde el PC.
    NoticeReply { key: String, text: String },
    /// Descartarla desde el PC.
    NoticeDismiss { key: String },
    /// Contestar o colgar la llamada desde el PC.
    CallAction(CallAction),
    /// El PC pide que suene el celular para encontrarlo; `false`: que se calle.
    Ring(bool),
    /// La respuesta a [`Phone::request_access`]. Si se aceptó, antes llega un
    /// [`PhoneEvent::Status`] conectado con las funciones nuevas.
    AccessAnswer { features: Vec<String>, granted: bool },
    /// Abrir un enlace del PC en el navegador; ya revisado con [`crate::is_web_url`].
    OpenUrl(String),
    /// El PC pide una foto: abrir la cámara y mandarla con [`Phone::send_photo`].
    TakePhoto,
    /// Lo nuevo que escribió el agente que se mira, por partes; todo si `reset`.
    /// Llega en vez de [`PhoneEvent::AgentOutput`] con un PC que tiene [`feature::AGENTS`].
    AgentEntries { agent_id: String, entries: Vec<crate::AgentEntry>, reset: bool },
    /// Lo nuevo que escribió el agente que se mira; todo si `reset`.
    AgentOutput { agent_id: String, lines: Vec<String>, reset: bool },
    /// Un archivo del PC, ya guardado en la carpeta de entrada.
    FileReceived { header: FileHeader, path: PathBuf },
    /// Avance de un archivo que se manda ([`Phone::send_file`]).
    FileProgress { id: String, sent: u64, total: u64 },
    FileSent { id: String },
    FileFailed { id: String, reason: String },
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
/// La conexión abierta y lo que sabe hacer ese PC; `None` sin sesión.
type Live = Arc<Mutex<Option<(Connection, Vec<String>)>>>;

/// La conexión con un PC: su tarea (que reconecta sola) y la cola de salida,
/// que solo existe mientras hay sesión abierta.
struct Session {
    outbox: Outbox,
    live: Live,
    events: mpsc::UnboundedSender<PhoneEvent>,
    task: JoinHandle<()>,
    /// Corta la espera entre intentos: la red cambió y vale la pena probar ya.
    wake: Arc<Notify>,
}

pub struct Phone {
    endpoint: Endpoint,
    device_name: String,
    inbox: Option<PathBuf>,
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
        crate::add_mdns(&endpoint);
        Ok(Self {
            endpoint,
            device_name: config.device_name,
            inbox: config.inbox,
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
        let live: Live = Arc::new(Mutex::new(None));
        let wake = Arc::new(Notify::new());
        let task = tokio::spawn(supervise(
            Ctx {
                endpoint: self.endpoint.clone(),
                device_name: self.device_name.clone(),
                inbox: self.inbox.clone(),
                events: events.clone(),
                outbox: outbox.clone(),
                live: live.clone(),
            },
            target,
            wake.clone(),
        ));
        self.sessions.lock().unwrap().insert(id.clone(), Session { outbox, live, events, task, wake });
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

    /// Abre un enlace en el navegador de ese PC, si tiene [`feature::LINKS`].
    /// Solo `http` y `https` ([`crate::is_web_url`]).
    pub fn open_url(&self, desktop: &str, url: String) -> bool {
        crate::is_web_url(&url) && self.supports(desktop, feature::LINKS) && self.send_to(desktop, ToDesktop::OpenUrl { url })
    }

    /// Pide a ese PC que prenda funciones que tiene apagadas
    /// ([`feature::requestable`]: escribirle a los agentes, controlarlo). En el
    /// PC aparece la pregunta; la respuesta llega como [`PhoneEvent::AccessAnswer`].
    /// `false` si ese PC no sabe recibir el pedido o no hay nada que pedir.
    pub fn request_access(&self, desktop: &str, features: Vec<String>) -> bool {
        let features: Vec<String> = features.into_iter().filter(|f| feature::requestable(f)).collect();
        !features.is_empty()
            && self.supports(desktop, feature::ACCESS)
            && self.send_to(desktop, ToDesktop::RequestAccess { features })
    }

    /// La foto que pidió ese PC ([`PhoneEvent::TakePhoto`]), JPEG o PNG.
    pub fn send_photo(&self, desktop: &str, mime: String, data: Vec<u8>) -> bool {
        !data.is_empty()
            && data.len() <= crate::proto::MAX_IMAGE_BYTES
            && self.supports(desktop, feature::PHOTO)
            && self.send_to(desktop, ToDesktop::Photo { mime, data })
    }

    /// Medios, detener la grabación o (si ese PC tiene [`feature::CONTROL`])
    /// volumen, bloquear y diapositivas. `false` si no salió.
    pub fn command(&self, desktop: &str, command: PcCommand) -> bool {
        if command.feature().is_some_and(|f| !self.supports(desktop, f)) {
            return false;
        }
        self.send_to(desktop, ToDesktop::Command { command })
    }

    /// El puntero o el teclado de ese PC, si tiene [`feature::REMOTE`].
    pub fn input(&self, desktop: &str, event: InputEvent) -> bool {
        if matches!(&event, InputEvent::Text { text } if text.is_empty() || text.len() > MAX_INPUT_TEXT) {
            return false;
        }
        self.supports(desktop, feature::REMOTE) && self.send_to(desktop, ToDesktop::Input { event })
    }

    /// Lo que sabe hacer un PC conectado (vacío si no está o es viejo).
    pub fn features(&self, desktop: &str) -> Vec<String> {
        let sessions = self.sessions.lock().unwrap();
        sessions
            .get(desktop)
            .and_then(|s| s.live.lock().unwrap().as_ref().map(|(_, f)| f.clone()))
            .unwrap_or_default()
    }

    pub fn supports(&self, desktop: &str, f: &str) -> bool {
        self.features(desktop).iter().any(|x| x == f)
    }

    /// Una notificación del celular, a los PCs que las muestran. Devuelve a cuántos salió.
    pub fn publish_notice(&self, notice: PhoneNotice) -> usize {
        self.broadcast_if(feature::NOTICES, ToDesktop::Notice { notice })
    }

    pub fn notice_removed(&self, key: String) -> usize {
        self.broadcast_if(feature::NOTICES, ToDesktop::NoticeRemoved { key })
    }

    /// La llamada en curso (o `None`), a los PCs que las muestran.
    pub fn publish_call(&self, call: Option<CallState>) -> usize {
        self.broadcast_if(feature::CALLS, ToDesktop::Call { call })
    }

    /// Le escribe a un agente de ese PC. `false` si el PC no sabe recibirlo.
    pub fn prompt(&self, desktop: &str, agent_id: String, text: String) -> bool {
        self.prompt_with(desktop, agent_id, text, Vec::new(), Vec::new())
    }

    /// Un mensaje con imágenes del celular (hasta [`crate::MAX_PROMPT_IMAGES`],
    /// [`crate::MAX_PROMPT_IMAGE_BYTES`] en total) o del historial del portapapeles
    /// de ese PC (`clip_ids`). Sin texto vale si lleva alguna. `false` si ese PC
    /// no las recibe ([`feature::AGENTS`]) o no caben.
    pub fn prompt_with(&self, desktop: &str, agent_id: String, text: String, images: Vec<crate::PromptImage>, clip_ids: Vec<String>) -> bool {
        let attached = !images.is_empty() || !clip_ids.is_empty();
        if attached && (!self.supports(desktop, feature::AGENTS) || !crate::prompt_images_ok(&images) || clip_ids.len() > crate::MAX_PROMPT_IMAGES) {
            return false;
        }
        (attached || !text.trim().is_empty())
            && self.supports(desktop, feature::PROMPT)
            && self.send_to(desktop, ToDesktop::Prompt { agent_id, text, images, clip_ids })
    }

    /// Aceptar, abrir en el PC o detener un agente. `false` si ese PC no lo
    /// sabe ([`feature::AGENTS`]; detener pide además [`feature::PROMPT`]).
    pub fn agent_action(&self, desktop: &str, agent_id: String, action: crate::AgentAction) -> bool {
        self.supports(desktop, feature::AGENTS)
            && (action != crate::AgentAction::Stop || self.supports(desktop, feature::PROMPT))
            && self.send_to(desktop, ToDesktop::AgentAction { agent_id, action })
    }

    /// Mirar en vivo un agente de ese PC; `None` para dejar de mirar.
    pub fn watch(&self, desktop: &str, agent_id: Option<String>) -> bool {
        self.supports(desktop, feature::WATCH) && self.send_to(desktop, ToDesktop::Watch { agent_id })
    }

    /// Manda un archivo a ese PC por su propia corriente. El avance llega por
    /// los eventos de ese PC ([`PhoneEvent::FileProgress`], `FileSent`,
    /// `FileFailed`). Devuelve el id del envío, o `None` si no se puede.
    /// Empieza a mandar video a ese PC (la pantalla del celular, ver [`crate::video`]).
    /// `None` si ese PC no está conectado o no muestra video. El video para al soltar
    /// el [`crate::video::VideoSender`]; lo que pide el PC llega a `on_control` y el
    /// final, a `on_end`. Hace falta estar dentro de un runtime de tokio.
    pub fn start_video(
        &self,
        desktop: &str,
        hello: crate::video::VideoHello,
        on_control: impl Fn(crate::video::VideoControl) + Send + Sync + 'static,
        on_end: impl FnOnce(Option<String>) + Send + 'static,
    ) -> Option<crate::video::VideoSender> {
        let remote = {
            let sessions = self.sessions.lock().unwrap();
            let session = sessions.get(desktop)?;
            let live = session.live.lock().unwrap();
            let (conn, features) = live.as_ref()?;
            if !features.iter().any(|f| f == feature::SCREEN) {
                return None;
            }
            conn.remote_id()
        };
        Some(crate::video::start_sender(self.endpoint.clone(), remote, hello, Arc::new(on_control), Box::new(on_end)))
    }

    pub fn send_file(&self, desktop: &str, path: &Path, name: String, mime: String, kind: FileKind, meta: Option<String>) -> Option<String> {
        let (conn, events) = {
            let sessions = self.sessions.lock().unwrap();
            let session = sessions.get(desktop)?;
            let live = session.live.lock().unwrap();
            let (conn, features) = live.as_ref()?;
            if !features.iter().any(|f| f == feature::FILES) {
                return None;
            }
            (conn.clone(), session.events.clone())
        };
        let size = std::fs::metadata(path).ok()?.len();
        let id = crate::ticket::new_token();
        let header = FileHeader { id: id.clone(), name, mime, size, kind, meta };
        let path = path.to_path_buf();
        let file_id = id.clone();
        tokio::spawn(async move {
            let progress = |sent, total| {
                let _ = events.send(PhoneEvent::FileProgress { id: file_id.clone(), sent, total });
            };
            let event = match crate::files::send(&conn, header, &path, progress).await {
                Ok(()) => PhoneEvent::FileSent { id: file_id.clone() },
                Err(e) => PhoneEvent::FileFailed { id: file_id.clone(), reason: format!("{e:#}") },
            };
            let _ = events.send(event);
        });
        Some(id)
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

    /// A los PCs conectados que tienen la función `f`.
    fn broadcast_if(&self, f: &str, msg: ToDesktop) -> usize {
        let sessions = self.sessions.lock().unwrap();
        sessions
            .values()
            .filter(|s| s.live.lock().unwrap().as_ref().is_some_and(|(_, fs)| fs.iter().any(|x| x == f)))
            .filter(|s| s.outbox.lock().unwrap().as_ref().is_some_and(|tx| tx.send(msg.clone()).is_ok()))
            .count()
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

/// Lo que necesita la tarea de un PC para conectarse y repartir lo que llega.
struct Ctx {
    endpoint: Endpoint,
    device_name: String,
    inbox: Option<PathBuf>,
    events: mpsc::UnboundedSender<PhoneEvent>,
    outbox: Outbox,
    live: Live,
}

async fn supervise(ctx: Ctx, mut target: Target, wake: Arc<Notify>) {
    let emit = |e: PhoneEvent| {
        let _ = ctx.events.send(e);
    };
    let mut backoff = Duration::from_secs(1);
    let mut rejected_wait = REJECTED_RETRY;
    loop {
        emit(PhoneEvent::Status(LinkStatus::Connecting));
        let mut welcomed = false;
        let ended = session(&ctx, &mut target, &mut welcomed).await;
        *ctx.outbox.lock().unwrap() = None;
        *ctx.live.lock().unwrap() = None;
        let reason = match ended {
            // Un ticket rechazado no sirve más. Un PC ya vinculado que no
            // reconoce al celular, en cambio, puede volver a hacerlo (se
            // restauró su lista, se abrió otra copia de la app): se sigue
            // probando, despacio, en vez de obligar a vincular de nuevo.
            Ok(Ended::Rejected(reason)) => {
                emit(PhoneEvent::Rejected(reason.clone()));
                emit(PhoneEvent::Status(LinkStatus::Offline { reason }));
                if matches!(target, Target::Ticket(_)) {
                    return;
                }
                tokio::select! {
                    _ = tokio::time::sleep(rejected_wait) => rejected_wait = (rejected_wait * 2).min(REJECTED_RETRY_MAX),
                    // Reintentar a mano o red nueva: se prueba ya y vuelve a ser seguido.
                    _ = wake.notified() => rejected_wait = REJECTED_RETRY,
                }
                backoff = Duration::from_secs(1);
                continue;
            }
            Ok(Ended::Closed) => "El PC cerró la conexión.".to_string(),
            Err(e) => format!("{e:#}"),
        };
        if welcomed {
            backoff = Duration::from_secs(1);
            rejected_wait = REJECTED_RETRY;
        }
        emit(PhoneEvent::Status(LinkStatus::Offline { reason }));
        tokio::select! {
            _ = tokio::time::sleep(backoff) => backoff = (backoff * 2).min(BACKOFF_MAX),
            // Con red nueva, la espera acumulada ya no dice nada.
            _ = wake.notified() => backoff = Duration::from_secs(1),
        }
    }
}

async fn session(ctx: &Ctx, target: &mut Target, welcomed: &mut bool) -> Result<Ended> {
    let events = &ctx.events;
    let emit = |e: PhoneEvent| {
        let _ = events.send(e);
    };
    let conn = tokio::time::timeout(CONNECT_TIMEOUT, ctx.endpoint.connect(target.addr(), ALPN))
        .await
        .context("El PC no respondió.")?
        .map_err(|e| anyhow!("No se pudo llegar al PC: {e:#}"))?;
    let (mut send, mut recv) = conn.open_bi().await?;

    // El celular anuncia todo lo que sabe; cada cosa nueva se usa solo si el PC también la anuncia.
    let hello = match target {
        Target::Ticket(t) => ToDesktop::Pair {
            token: t.token.clone(),
            device_name: ctx.device_name.clone(),
            features: feature::all(),
        },
        Target::Known(_) => ToDesktop::Hello { device_name: ctx.device_name.clone(), features: feature::all() },
    };
    write_frame(&mut send, &hello).await?;

    // Un PC que acepta la conexión y se queda mudo no deja al celular esperando para siempre.
    let mut first = tokio::time::timeout(Duration::from_secs(15), read_frame(&mut recv))
        .await
        .context("El PC no respondió.")??
        .context("El PC cerró sin responder.")?;
    if first == ToPhone::AwaitingApproval {
        emit(PhoneEvent::Status(LinkStatus::AwaitingApproval));
        // El PC espera dos minutos a que alguien acepte; un poco más acá.
        first = tokio::time::timeout(Duration::from_secs(150), read_frame(&mut recv))
            .await
            .context("Nadie aceptó en el PC.")??
            .context("El PC cerró sin responder.")?;
    }
    let (desktop_name, features) = match first {
        ToPhone::Welcome { desktop_name, features } => {
            if let Target::Ticket(t) = target {
                let desktop = PairedDesktop { addr: t.addr.clone(), name: desktop_name.clone() };
                emit(PhoneEvent::Paired(desktop.clone()));
                // Desde acá, reconectar es saludar: el token ya se gastó.
                *target = Target::Known(desktop);
            }
            *welcomed = true;
            *ctx.live.lock().unwrap() = Some((conn.clone(), features.clone()));
            emit(PhoneEvent::Status(LinkStatus::Connected { desktop_name: desktop_name.clone(), features: features.clone() }));
            (desktop_name, features)
        }
        ToPhone::Rejected { reason } => return Ok(Ended::Rejected(reason)),
        other => bail!("el PC mandó {other:?} antes de saludar"),
    };
    let files = features.iter().any(|f| f == feature::FILES);

    let (tx, mut rx) = mpsc::unbounded_channel();
    *ctx.outbox.lock().unwrap() = Some(tx);
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
                Some(ToPhone::NoticeReply { key, text }) => emit(PhoneEvent::NoticeReply { key, text }),
                Some(ToPhone::NoticeDismiss { key }) => emit(PhoneEvent::NoticeDismiss { key }),
                Some(ToPhone::CallAction { action }) => emit(PhoneEvent::CallAction(action)),
                Some(ToPhone::Ring { on }) => emit(PhoneEvent::Ring(on)),
                Some(ToPhone::OpenUrl { url }) if crate::is_web_url(&url) => emit(PhoneEvent::OpenUrl(url)),
                Some(ToPhone::TakePhoto) => emit(PhoneEvent::TakePhoto),
                // Se prendió o apagó algo en el PC: el mismo aviso de conectado,
                // con lo nuevo, y lo que se puede mandar cambia desde ya.
                Some(ToPhone::Features { features }) => {
                    if let Some((_, current)) = ctx.live.lock().unwrap().as_mut() {
                        *current = features.clone();
                    }
                    emit(PhoneEvent::Status(LinkStatus::Connected { desktop_name: desktop_name.clone(), features }));
                }
                Some(ToPhone::AgentEntries { agent_id, entries, reset }) => emit(PhoneEvent::AgentEntries { agent_id, entries, reset }),
                Some(ToPhone::AccessAnswer { features, granted }) => emit(PhoneEvent::AccessAnswer { features, granted }),
                Some(ToPhone::AgentOutput { agent_id, lines, reset }) => {
                    emit(PhoneEvent::AgentOutput { agent_id, lines, reset })
                }
                Some(_) => {}
                None => return Ok(Ended::Closed),
            },
            // Un archivo del PC: cada uno en su tarea, para no frenar los mensajes.
            stream = conn.accept_uni() => {
                let mut stream = stream?;
                match (&ctx.inbox, files) {
                    (Some(dir), true) => {
                        let dir = dir.clone();
                        let events = events.clone();
                        tokio::spawn(async move {
                            match crate::files::receive(stream, &dir).await {
                                Ok((header, path)) => {
                                    let _ = events.send(PhoneEvent::FileReceived { header, path });
                                }
                                Err(e) => tracing::debug!("archivo del PC a medias: {e:#}"),
                            }
                        });
                    }
                    _ => {
                        let _ = stream.stop(0u32.into());
                    }
                }
            }
            Some(out) = rx.recv() => write_frame(&mut send, &out).await?,
        }
    }
}
