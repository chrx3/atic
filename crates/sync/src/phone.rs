//! El lado del celular: se conecta a sus escritorios, reconecta solo y manda
//! decisiones.
//!
//! Un celular puede estar pareado con varios PCs a la vez: una sesión por PC,
//! todas sobre el mismo endpoint. El celular es el centro: lo que llega de un
//! PC, quien hospeda lo reparte a los otros (ver `SyncLink` en Android).

use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, atomic::{AtomicU64, AtomicUsize, Ordering}},
    time::Duration,
};

use anyhow::{anyhow, bail, Context, Result};
use iroh::{
    endpoint::{presets, Connection},
    Endpoint, EndpointAddr, SecretKey,
};
use serde::{Deserialize, Serialize};
use tokio::{
    sync::{mpsc, oneshot, Notify},
    task::JoinHandle,
};

use crate::{
    lock::MutexExt,
    proto::{
        read_frame, write_frame, AgentCard, ClipItem, Decision, PcCommand, PcState, ToDesktop, ToPhone, ALPN,
        MAX_CLIP_BYTES,
    },
    proto::{feature, CallAction, CallState, FileHeader, FileKind, InputEvent, PhoneNotice, MAX_FILE_BYTES, MAX_INPUT_TEXT},
    ticket::PairingTicket,
};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const BACKOFF_MAX: Duration = Duration::from_secs(30);
/// Cada cuánto se vuelve a probar con un PC que dijo no reconocer al celular:
/// al principio cada 2 minutos, y cada vez más espaciado hasta media hora (si
/// lo desvincularon a propósito, no tiene sentido insistir seguido).
const REJECTED_RETRY: Duration = Duration::from_secs(120);
const REJECTED_RETRY_MAX: Duration = Duration::from_secs(30 * 60);
/// Archivos ofrecidos por un PC que esperan a la vez la respuesta del usuario: uno
/// más se rechaza solo, para que un PC no llene el celular de preguntas.
const MAX_PENDING_OFFERS: usize = 8;
/// Tope del id de un archivo ofrecido: vuelve en la respuesta.
const MAX_FILE_ID: usize = 128;

pub struct PhoneConfig {
    pub secret_key: [u8; 32],
    /// Cómo aparece en el escritorio ("Galaxy S24 Ultra").
    pub device_name: String,
    /// Dónde quedan los archivos que mandan los PCs. Sin carpeta no se reciben.
    pub inbox: Option<PathBuf>,
    /// La instalación de la app ([`ToDesktop::Pair`]): con ella el PC reconoce al
    /// mismo celular aunque la app se haya reinstalado y tenga otra clave.
    pub install_id: Option<String>,
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
    /// Entrada del PC sobre la pantalla compartida.
    ScreenInput(InputEvent),
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
    /// El PC pide ver la pantalla del celular. Con clave, también abrir la app de ese aviso
    /// una vez que el usuario acepte compartir.
    ShareScreen { open: Option<String> },
    /// El PC pide controlar el celular: preguntarle al usuario y, si acepta, habilitar el control.
    ControlRequested,
    /// Lo nuevo que escribió el agente que se mira, por partes; todo si `reset`.
    /// Llega en vez de [`PhoneEvent::AgentOutput`] con un PC que tiene [`feature::AGENTS`].
    AgentEntries { agent_id: String, entries: Vec<crate::AgentEntry>, reset: bool },
    /// Lo nuevo que escribió el agente que se mira; todo si `reset`.
    AgentOutput { agent_id: String, lines: Vec<String>, reset: bool },
    /// Un archivo del PC, ya guardado en la carpeta de entrada.
    FileReceived { header: FileHeader, path: PathBuf },
    /// El PC ofrece un archivo que este celular no pidió ([`feature::FILE_OFFER`]): todavía
    /// no llegó nada de su contenido. Se contesta con [`Phone::answer_file`]; el nombre
    /// ya viene limpio ([`crate::proto::safe_file_name`]). Si se acepta, después llega
    /// [`PhoneEvent::FileReceived`] con el mismo `id`.
    FileOffered { header: FileHeader },
    /// El ofrecimiento de ese `id` ya no espera respuesta (el PC lo retiró, se cortó la
    /// conexión o pasó el plazo): lo que se le preguntaba al usuario sobra.
    FileOfferClosed { id: String },
    /// Avance de un archivo que se manda ([`Phone::send_file`]).
    FileProgress { id: String, sent: u64, total: u64 },
    FileSent { id: String },
    FileFailed { id: String, reason: String },
    /// Avance de un archivo que llega del PC (el `id` de su [`FileHeader`]; en un
    /// archivo de proyecto, el `req_id` de [`Phone::fetch_file`]).
    FileReceiving { id: String, name: String, received: u64, total: u64 },
    /// Respuesta a [`Phone::list_projects`].
    Projects {
        projects: Vec<crate::ProjectInfo>,
        agents: Vec<crate::Choice>,
        models: Vec<crate::Choice>,
        efforts: Vec<crate::Choice>,
        modes: Vec<crate::Choice>,
        bypass: bool,
    },
    /// Respuesta a [`Phone::new_chat`].
    ChatCreated { req_id: String, ok: bool, reason: Option<String>, chat_key: Option<String> },
    /// Respuesta a [`Phone::list_changes`].
    Changes { agent_id: String, root_name: String, branch: Option<String>, files: Vec<crate::ChangedFile> },
    /// Respuesta a [`Phone::fetch_file`]; con `ok`, después llega el archivo
    /// ([`PhoneEvent::FileReceived`] con `header.id == req_id`).
    FetchResult { req_id: String, ok: bool, reason: Option<String>, size: u64, name: String, mime: String },
    /// Un agente terminó un turno con un artefacto nuevo; se baja con [`Phone::fetch_file`].
    ArtifactReady { agent_id: String, rel_path: String, size: u64, mime: String },
    /// Lo que se pidió sobre un agente no se pudo hacer.
    CodeError { agent_id: Option<String>, reason: String },
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
/// Los archivos ofrecidos por un PC que esperan la respuesta de quien hospeda, por id.
type Offers = Arc<Mutex<HashMap<String, oneshot::Sender<bool>>>>;
/// Los archivos que este celular le pidió a un PC ([`Phone::fetch_file`]) y todavía
/// no llegan: esos no se le preguntan a nadie.
type Requested = Arc<Mutex<HashSet<String>>>;

/// Qué PC puede manejar la pantalla que comparte el celular.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct ScreenControl {
    enabled: bool,
    /// Solo este PC ([`PairedDesktop::id`]); `None`: cualquiera de los vinculados.
    only: Option<String>,
}

impl ScreenControl {
    fn allows(&self, desktop: &str) -> bool {
        self.enabled && self.only.as_deref().is_none_or(|only| only == desktop)
    }
}
/// La conexión abierta y lo que sabe hacer ese PC; `None` sin sesión.
type Live = Arc<Mutex<Option<(Connection, Vec<String>)>>>;

/// La conexión con un PC: su tarea (que reconecta sola) y la cola de salida,
/// que solo existe mientras hay sesión abierta.
struct Session {
    outbox: Outbox,
    live: Live,
    sharing: Arc<AtomicUsize>,
    answers: crate::files::Answers,
    offers: Offers,
    requested: Requested,
    events: mpsc::UnboundedSender<PhoneEvent>,
    task: JoinHandle<()>,
    /// Corta la espera entre intentos: la red cambió y vale la pena probar ya.
    wake: Arc<Notify>,
}

pub struct Phone {
    endpoint: Endpoint,
    device_name: String,
    install_id: Option<String>,
    screen_control: Arc<Mutex<ScreenControl>>,
    inbox: Option<PathBuf>,
    /// Tope de un archivo que manda un PC ([`Phone::set_file_limit`]).
    file_limit: Arc<AtomicU64>,
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
            install_id: config.install_id,
            screen_control: Default::default(),
            inbox: config.inbox,
            file_limit: Arc::new(AtomicU64::new(MAX_FILE_BYTES)),
            sessions: Mutex::new(HashMap::new()),
        })
    }

    /// Tope de un archivo que manda un PC, en bytes (nunca más que
    /// [`crate::MAX_FILE_BYTES`]). Se revisa con el encabezado, antes de escribir nada.
    pub fn set_file_limit(&self, bytes: u64) {
        self.file_limit.store(bytes.min(MAX_FILE_BYTES), Ordering::Relaxed);
    }

    /// Habilita el control para todos los PCs y actualiza las sesiones compatibles.
    pub fn set_screen_control(&self, enabled: bool) {
        self.apply_screen_control(ScreenControl { enabled, only: None });
    }

    /// Habilita el control solo para ese PC (el que recibe la pantalla): a los demás
    /// se les anuncia apagado y su entrada se descarta. `None`: para ninguno.
    pub fn set_screen_control_for(&self, desktop: Option<&str>) {
        self.apply_screen_control(ScreenControl { enabled: desktop.is_some(), only: desktop.map(str::to_string) });
    }

    /// Cada PC se entera de lo suyo, con el mensaje de siempre: solo los que anuncian
    /// [`feature::SCREEN_CONTROL`] lo conocen.
    fn apply_screen_control(&self, control: ScreenControl) {
        *self.screen_control.lock_or_recover() = control.clone();
        for (id, session) in self.sessions.lock_or_recover().iter() {
            let compatible = session.live.lock_or_recover().as_ref().is_some_and(|(_, fs)| fs.iter().any(|f| f == feature::SCREEN_CONTROL));
            if compatible {
                if let Some(tx) = session.outbox.lock_or_recover().as_ref() {
                    let _ = tx.send(ToDesktop::Features { features: phone_features(control.allows(id)) });
                }
            }
        }
    }

    /// El endpoint de este celular, para [`crate::peer::Peers`]: misma identidad.
    pub fn endpoint(&self) -> Endpoint {
        self.endpoint.clone()
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
        let sharing = Arc::new(AtomicUsize::new(0));
        let answers = crate::files::Answers::default();
        let offers = Offers::default();
        let requested = Requested::default();
        let task = tokio::spawn(supervise(
            Ctx {
                endpoint: self.endpoint.clone(),
                desktop: id.clone(),
                device_name: self.device_name.clone(),
                install_id: self.install_id.clone(),
                screen_control: self.screen_control.clone(),
                inbox: self.inbox.clone(),
                file_limit: self.file_limit.clone(),
                events: events.clone(),
                outbox: outbox.clone(),
                live: live.clone(),
                sharing: sharing.clone(),
                answers: answers.clone(),
                offers: offers.clone(),
                requested: requested.clone(),
            },
            target,
            wake.clone(),
        ));
        self.sessions.lock_or_recover().insert(id.clone(), Session { outbox, live, sharing, answers, offers, requested, events, task, wake });
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
        for session in self.sessions.lock_or_recover().values() {
            if session.outbox.lock_or_recover().is_none() {
                session.wake.notify_one();
            }
        }
    }

    /// Los PCs con sesión abierta ahora.
    pub fn connected(&self) -> Vec<String> {
        self.sessions
            .lock_or_recover()
            .iter()
            .filter(|(_, s)| s.outbox.lock_or_recover().is_some())
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

    /// Le cuenta a ese PC qué pasó con el enlace que mandó a abrir
    /// ([`PhoneEvent::OpenUrl`]): se abrió o el usuario dijo que no. `false` si ese PC
    /// no lo sabe recibir ([`feature::LINK_RESULT`]).
    pub fn url_result(&self, desktop: &str, url: String, opened: bool) -> bool {
        crate::is_web_url(&url) && self.supports(desktop, feature::LINK_RESULT) && self.send_to(desktop, ToDesktop::UrlResult { url, opened })
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
        let sessions = self.sessions.lock_or_recover();
        sessions
            .get(desktop)
            .and_then(|s| s.live.lock_or_recover().as_ref().map(|(_, f)| f.clone()))
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

    /// Empieza a mandar video a ese PC (la pantalla del celular, ver [`crate::video`]).
    /// `None` si ese PC no está conectado o no muestra video. El video para al soltar
    /// el [`crate::video::VideoSender`]; lo que pide el PC (y el bitrate que decide la
    /// conexión, sin pasar de `max_bitrate`) llega a `on_control` y el final, a `on_end`. Hace falta estar dentro de un runtime de tokio.
    pub fn start_video(
        &self,
        desktop: &str,
        hello: crate::video::VideoHello,
        max_bitrate: u32,
        on_control: impl Fn(crate::video::VideoControl) + Send + Sync + 'static,
        on_end: impl FnOnce(Option<String>) + Send + 'static,
    ) -> Option<crate::video::VideoSender> {
        let remote = {
            let sessions = self.sessions.lock_or_recover();
            let session = sessions.get(desktop)?;
            let live = session.live.lock_or_recover();
            let (conn, features) = live.as_ref()?;
            if !features.iter().any(|f| f == feature::SCREEN) {
                return None;
            }
            conn.remote_id()
        };
        let sharing = self.sessions.lock_or_recover().get(desktop)?.sharing.clone();
        let screen = hello.source == crate::video::VideoSource::Screen;
        if screen { sharing.fetch_add(1, Ordering::SeqCst); }
        Some(crate::video::start_sender(self.endpoint.clone(), remote, hello, max_bitrate, Arc::new(on_control), Box::new(move |reason| {
            if screen { sharing.fetch_sub(1, Ordering::SeqCst); }
            on_end(reason);
        })))
    }

    /// Pide la pantalla de ese PC (`monitor`: cuál de las suyas; `None`, la principal), a
    /// `max_width` de ancho como mucho. Al aceptar, `on_ready` recibe las pantallas del PC
    /// y cuál comparte. Cada cuadro llega a `on_frame` (H.264 Annex-B, con FORMAT al empezar) y el final a `on_end`. `None`
    /// si ese PC no está conectado o no comparte su pantalla. Soltar la vista la detiene.
    /// Hace falta estar dentro de un runtime de tokio.
    pub fn watch_screen(
        &self,
        desktop: &str,
        max_width: u32,
        monitor: Option<u32>,
        on_ready: impl FnOnce(Vec<crate::video::ScreenInfo>, u32) + Send + 'static,
        on_frame: impl Fn(crate::video::VideoFrame) + Send + Sync + 'static,
        on_end: impl FnOnce(Option<String>) + Send + 'static,
    ) -> Option<crate::video::ScreenView> {
        let remote = {
            let sessions = self.sessions.lock_or_recover();
            let session = sessions.get(desktop)?;
            let live = session.live.lock_or_recover();
            let (conn, features) = live.as_ref()?;
            if !features.iter().any(|f| f == feature::SCREEN_OUT) {
                return None;
            }
            conn.remote_id()
        };
        Some(crate::video::start_watcher(self.endpoint.clone(), remote, max_width, monitor, Box::new(on_ready), Box::new(on_frame), Box::new(on_end)))
    }

    /// Pide a ese PC sus proyectos de Atic Code; llega [`PhoneEvent::Projects`].
    /// `false` si ese PC no lo sabe ([`feature::PROJECTS`]).
    pub fn list_projects(&self, desktop: &str) -> bool {
        self.supports(desktop, feature::PROJECTS) && self.send_to(desktop, ToDesktop::ListProjects)
    }

    /// Una conversación nueva en un proyecto de ese PC (`project_id` de
    /// [`crate::ProjectInfo::id`]), con su primer mensaje. `agent`: `claude` (o `None`)
    /// u otro de los que trae [`PhoneEvent::Projects`]. La respuesta llega como
    /// [`PhoneEvent::ChatCreated`] con `req_id`. `false` si ese PC no lo permite
    /// ([`feature::PROJECTS`] y [`feature::PROMPT`]) o no hay mensaje.
    #[allow(clippy::too_many_arguments)]
    pub fn new_chat(
        &self,
        desktop: &str,
        req_id: String,
        project_id: String,
        agent: Option<String>,
        model: Option<String>,
        text: String,
        images: Vec<crate::PromptImage>,
    ) -> bool {
        (!text.trim().is_empty() || !images.is_empty())
            && !req_id.is_empty()
            && crate::prompt_images_ok(&images)
            && self.supports(desktop, feature::PROJECTS)
            && self.supports(desktop, feature::PROMPT)
            && self.send_to(desktop, ToDesktop::NewChat { req_id, project_id, agent, model, text, images })
    }

    /// El modelo (vacío: el predeterminado) y, si viene, el esfuerzo de una conversación.
    /// Se ve en la tarjeta cuando se aplica; si no se puede, [`PhoneEvent::CodeError`].
    pub fn set_model(&self, desktop: &str, agent_id: String, model: String, effort: Option<String>) -> bool {
        self.supports(desktop, feature::PROJECTS)
            && self.supports(desktop, feature::PROMPT)
            && self.send_to(desktop, ToDesktop::SetModel { agent_id, model, effort })
    }

    /// El modo de permisos de una conversación (`default`, `acceptEdits`, `plan`,
    /// `bypassPermissions`). Se ve en la tarjeta; si no se puede (el PC no permite
    /// `bypassPermissions`), [`PhoneEvent::CodeError`].
    pub fn set_mode(&self, desktop: &str, agent_id: String, mode: String) -> bool {
        !mode.is_empty()
            && self.supports(desktop, feature::PROJECTS)
            && self.supports(desktop, feature::PROMPT)
            && self.send_to(desktop, ToDesktop::SetMode { agent_id, mode })
    }

    /// Lo que cambió en el proyecto de un agente; llega [`PhoneEvent::Changes`].
    pub fn list_changes(&self, desktop: &str, agent_id: String) -> bool {
        self.supports(desktop, feature::PROJECTS) && self.send_to(desktop, ToDesktop::ListChanges { agent_id })
    }

    /// Baja un archivo del proyecto de un agente: una ruta de la última
    /// [`PhoneEvent::Changes`] o de un [`PhoneEvent::ArtifactReady`]. Llega
    /// [`PhoneEvent::FetchResult`] y, si se puede, el archivo con `header.id == req_id`
    /// (con su avance en [`PhoneEvent::FileReceiving`]). `false` si ese PC no lo permite
    /// ([`feature::PROJECT_FILES`] y [`feature::FILES`]) o este celular no tiene carpeta.
    pub fn fetch_file(&self, desktop: &str, agent_id: String, rel_path: String, req_id: String) -> bool {
        let allowed = self.inbox.is_some()
            && !req_id.is_empty()
            && crate::project::clean_parts(&rel_path).is_some()
            && self.supports(desktop, feature::PROJECT_FILES)
            && self.supports(desktop, feature::FILES);
        if !allowed {
            return false;
        }
        // Anotado antes de pedirlo: el archivo puede llegar apenas sale el pedido, y
        // lo pedido no se le pregunta al usuario ([`feature::FILE_OFFER`]).
        let Some(requested) = self.sessions.lock_or_recover().get(desktop).map(|s| s.requested.clone()) else {
            return false;
        };
        requested.lock_or_recover().insert(req_id.clone());
        let sent = self.send_to(desktop, ToDesktop::FetchFile { agent_id, rel_path, req_id: req_id.clone() });
        if !sent {
            requested.lock_or_recover().remove(&req_id);
        }
        sent
    }

    /// La respuesta a un [`PhoneEvent::FileOffered`] de ese PC: aceptarlo (su contenido
    /// empieza a llegar) o no (no llega nada). `false` si ese ofrecimiento ya no espera
    /// respuesta.
    pub fn answer_file(&self, desktop: &str, id: &str, accept: bool) -> bool {
        let offer = {
            let sessions = self.sessions.lock_or_recover();
            sessions.get(desktop).and_then(|s| s.offers.lock_or_recover().remove(id))
        };
        offer.is_some_and(|tx| tx.send(accept).is_ok())
    }

    /// Manda un archivo a ese PC por su propia corriente. El avance llega por
    /// los eventos de ese PC ([`PhoneEvent::FileProgress`], `FileSent`,
    /// `FileFailed`). Devuelve el id del envío, o `None` si no se puede. Con un PC que
    /// tiene [`feature::FILE_OFFER`] se ofrece primero y el contenido sale cuando acepta.
    pub fn send_file(&self, desktop: &str, path: &Path, name: String, mime: String, kind: FileKind, meta: Option<String>) -> Option<String> {
        let (conn, events, answers) = {
            let sessions = self.sessions.lock_or_recover();
            let session = sessions.get(desktop)?;
            let live = session.live.lock_or_recover();
            let (conn, features) = live.as_ref()?;
            if !features.iter().any(|f| f == feature::FILES) {
                return None;
            }
            let answers = features.iter().any(|f| f == feature::FILE_OFFER).then(|| session.answers.clone());
            (conn.clone(), session.events.clone(), answers)
        };
        let size = std::fs::metadata(path).ok()?.len();
        let id = crate::ticket::new_token();
        let header = FileHeader { id: id.clone(), name, mime, size, kind, meta, offer: false };
        let path = path.to_path_buf();
        let file_id = id.clone();
        tokio::spawn(async move {
            let progress = |sent, total| {
                let _ = events.send(PhoneEvent::FileProgress { id: file_id.clone(), sent, total });
            };
            let sent = match answers {
                Some(answers) => crate::files::send_offered(&conn, header, &path, &answers, progress).await,
                None => crate::files::send(&conn, header, &path, progress).await,
            };
            let event = match sent {
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
        let sessions = self.sessions.lock_or_recover();
        sessions
            .get(desktop)
            .is_some_and(|s| s.outbox.lock_or_recover().as_ref().is_some_and(|tx| tx.send(msg).is_ok()))
    }

    /// A los PCs conectados que tienen la función `f`.
    fn broadcast_if(&self, f: &str, msg: ToDesktop) -> usize {
        let sessions = self.sessions.lock_or_recover();
        sessions
            .values()
            .filter(|s| s.live.lock_or_recover().as_ref().is_some_and(|(_, fs)| fs.iter().any(|x| x == f)))
            .filter(|s| s.outbox.lock_or_recover().as_ref().is_some_and(|tx| tx.send(msg.clone()).is_ok()))
            .count()
    }

    /// A todos los PCs con sesión abierta. Devuelve a cuántos salió.
    fn broadcast(&self, msg: ToDesktop) -> usize {
        let sessions = self.sessions.lock_or_recover();
        sessions
            .values()
            .filter(|s| s.outbox.lock_or_recover().as_ref().is_some_and(|tx| tx.send(msg.clone()).is_ok()))
            .count()
    }

    /// Deja de hablar con un PC (desvincularlo, o reconectar ya).
    pub fn disconnect(&self, desktop: &str) {
        if let Some(session) = self.sessions.lock_or_recover().remove(desktop) {
            session.task.abort();
        }
    }

    pub fn disconnect_all(&self) {
        for (_, session) in self.sessions.lock_or_recover().drain() {
            session.task.abort();
        }
    }

    pub async fn shutdown(self) {
        self.disconnect_all();
        self.endpoint.close().await;
    }
}

/// Lo que necesita la tarea que recibe un archivo de un PC.
struct Receiving {
    dir: PathBuf,
    events: mpsc::UnboundedSender<PhoneEvent>,
    /// Por dónde sale la respuesta a un archivo ofrecido.
    outbox: mpsc::UnboundedSender<ToDesktop>,
    /// Ese PC tiene [`feature::FILE_OFFER`]: lo que ofrece se contesta.
    offers_on: bool,
    /// Tope de un archivo, en bytes.
    limit: u64,
    offers: Offers,
    requested: Requested,
}

/// Un archivo del PC. Uno ofrecido ([`FileHeader::offer`]) se decide con el encabezado
/// antes de leer su contenido: lo que pasa el tope no se recibe, lo que pidió este
/// celular sigue de corrido y lo demás se le pregunta a quien hospeda. Uno sin
/// ofrecer (un PC viejo) llega entero como siempre.
async fn receive_file(mut stream: iroh::endpoint::RecvStream, r: Receiving) -> Result<()> {
    let mut header = crate::files::read_header(&mut stream).await?;
    let too_large = header.size > r.limit;
    // Llegó (o se ofrece): ya no está pendiente, con un PC nuevo o con uno viejo.
    let requested = r.requested.lock_or_recover().remove(&header.id);
    if header.offer && r.offers_on {
        if header.id.is_empty() || header.id.len() > MAX_FILE_ID {
            crate::files::refuse(&mut stream, false);
            bail!("archivo ofrecido con un id fuera de tope");
        }
        let answer = |accept: bool| {
            let _ = r.outbox.send(ToDesktop::FileAnswer { id: header.id.clone(), accept, too_large: !accept && too_large });
        };
        if too_large || !(requested || offer_accepted(&mut stream, &header, &r).await) {
            answer(false);
            crate::files::refuse(&mut stream, too_large);
            return Ok(());
        }
        answer(true);
    } else if too_large {
        bail!("el archivo pasa el tope");
    }
    // Ya se usó para decidir: lo que llega entero no es un ofrecimiento.
    header.offer = false;
    let events = r.events.clone();
    let progress = move |header: &FileHeader, received| {
        let name = crate::proto::safe_file_name(&header.name);
        let _ = events.send(PhoneEvent::FileReceiving { id: header.id.clone(), name, received, total: header.size });
    };
    let path = crate::files::receive_body(stream, &header, &r.dir, progress).await?;
    let _ = r.events.send(PhoneEvent::FileReceived { header, path });
    Ok(())
}

/// Le pregunta a quien hospeda por un archivo que nadie pidió y espera su respuesta
/// ([`Phone::answer_file`]). Sin respuesta (el PC lo retiró, se cortó, pasó el plazo o
/// ya hay demasiadas preguntas a la vez) es un no.
async fn offer_accepted(stream: &mut iroh::endpoint::RecvStream, header: &FileHeader, r: &Receiving) -> bool {
    let (tx, rx) = oneshot::channel();
    {
        let mut offers = r.offers.lock_or_recover();
        if offers.len() >= MAX_PENDING_OFFERS {
            return false;
        }
        offers.insert(header.id.clone(), tx);
    }
    let shown = FileHeader { name: crate::proto::safe_file_name(&header.name), offer: false, ..header.clone() };
    let _ = r.events.send(PhoneEvent::FileOffered { header: shown });
    let answered = tokio::select! {
        answered = rx => answered.ok(),
        // Quien lo ofrecía se cansó de esperar, o se cortó la conexión.
        _ = stream.received_reset() => None,
        _ = tokio::time::sleep(crate::files::OFFER_TIMEOUT + Duration::from_secs(30)) => None,
    };
    if answered.is_none() {
        r.offers.lock_or_recover().remove(&header.id);
        let _ = r.events.send(PhoneEvent::FileOfferClosed { id: header.id.clone() });
    }
    answered.unwrap_or(false)
}

fn screen_input_allowed(enabled: bool, sharing: bool, event: &InputEvent) -> bool {
    enabled && sharing && event.valid()
}

fn phone_features(control: bool) -> Vec<String> {
    feature::all().into_iter().filter(|f| control || f != feature::SCREEN_CONTROL).collect()
}

enum Ended {
    Closed,
    Rejected(String),
}

/// Lo que necesita la tarea de un PC para conectarse y repartir lo que llega.
struct Ctx {
    endpoint: Endpoint,
    /// El id del PC de esta tarea ([`PairedDesktop::id`]).
    desktop: String,
    device_name: String,
    install_id: Option<String>,
    screen_control: Arc<Mutex<ScreenControl>>,
    inbox: Option<PathBuf>,
    file_limit: Arc<AtomicU64>,
    events: mpsc::UnboundedSender<PhoneEvent>,
    outbox: Outbox,
    live: Live,
    sharing: Arc<AtomicUsize>,
    answers: crate::files::Answers,
    offers: Offers,
    requested: Requested,
}

impl Ctx {
    /// Si este PC puede manejar la pantalla del celular ahora.
    fn control(&self) -> bool {
        self.screen_control.lock_or_recover().allows(&self.desktop)
    }
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
        *ctx.outbox.lock_or_recover() = None;
        *ctx.live.lock_or_recover() = None;
        // Lo que esperaba respuesta de esta sesión ya no la va a tener.
        ctx.answers.clear();
        ctx.offers.lock_or_recover().clear();
        ctx.requested.lock_or_recover().clear();
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
            features: phone_features(ctx.control()),
            install_id: ctx.install_id.clone(),
        },
        Target::Known(_) => ToDesktop::Hello {
            device_name: ctx.device_name.clone(),
            features: phone_features(ctx.control()),
            install_id: ctx.install_id.clone(),
        },
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
            *ctx.live.lock_or_recover() = Some((conn.clone(), features.clone()));
            emit(PhoneEvent::Status(LinkStatus::Connected { desktop_name: desktop_name.clone(), features: features.clone() }));
            (desktop_name, features)
        }
        ToPhone::Rejected { reason } => return Ok(Ended::Rejected(reason)),
        other => bail!("el PC mandó {other:?} antes de saludar"),
    };

    let (tx, mut rx) = mpsc::unbounded_channel();
    *ctx.outbox.lock_or_recover() = Some(tx.clone());
    if features.iter().any(|f| f == feature::SCREEN_CONTROL) {
        write_frame(&mut send, &ToDesktop::Features { features: phone_features(ctx.control()) }).await?;
    }
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
                Some(ToPhone::Input { event }) if screen_input_allowed(ctx.control(), ctx.sharing.load(Ordering::SeqCst) > 0, &event) => emit(PhoneEvent::ScreenInput(event)),
                Some(ToPhone::TakePhoto) => emit(PhoneEvent::TakePhoto),
                Some(ToPhone::RequestControl) => emit(PhoneEvent::ControlRequested),
                Some(ToPhone::ShareScreen { open }) => emit(PhoneEvent::ShareScreen { open }),
                // Se prendió o apagó algo en el PC: el mismo aviso de conectado,
                // con lo nuevo, y lo que se puede mandar cambia desde ya.
                Some(ToPhone::Features { features }) => {
                    if features.iter().any(|f| f == feature::SCREEN_CONTROL) {
                        write_frame(&mut send, &ToDesktop::Features { features: phone_features(ctx.control()) }).await?;
                    }
                    if let Some((_, current)) = ctx.live.lock_or_recover().as_mut() {
                        *current = features.clone();
                    }
                    emit(PhoneEvent::Status(LinkStatus::Connected { desktop_name: desktop_name.clone(), features }));
                }
                Some(ToPhone::AgentEntries { agent_id, entries, reset }) => emit(PhoneEvent::AgentEntries { agent_id, entries, reset }),
                Some(ToPhone::AccessAnswer { features, granted }) => emit(PhoneEvent::AccessAnswer { features, granted }),
                Some(ToPhone::AgentOutput { agent_id, lines, reset }) => {
                    emit(PhoneEvent::AgentOutput { agent_id, lines, reset })
                }
                Some(ToPhone::Projects { projects, agents, models, efforts, modes, bypass }) => {
                    emit(PhoneEvent::Projects { projects, agents, models, efforts, modes, bypass })
                }
                Some(ToPhone::ChatCreated { req_id, ok, reason, chat_key }) => emit(PhoneEvent::ChatCreated { req_id, ok, reason, chat_key }),
                Some(ToPhone::Changes { agent_id, root_name, branch, files }) => emit(PhoneEvent::Changes { agent_id, root_name, branch, files }),
                Some(ToPhone::FetchResult { req_id, ok, reason, size, name, mime }) => {
                    if !ok {
                        ctx.requested.lock_or_recover().remove(&req_id);
                    }
                    emit(PhoneEvent::FetchResult { req_id, ok, reason, size, name: crate::proto::safe_file_name(&name), mime })
                }
                Some(ToPhone::ArtifactReady { agent_id, rel_path, size, mime }) if crate::project::clean_parts(&rel_path).is_some() => {
                    emit(PhoneEvent::ArtifactReady { agent_id, rel_path, size, mime })
                }
                Some(ToPhone::CodeError { agent_id, reason }) => emit(PhoneEvent::CodeError { agent_id, reason }),
                Some(ToPhone::FileAnswer { id, accept, too_large }) => ctx.answers.deliver(&id, crate::files::FileAnswer { accept, too_large }),
                Some(_) => {}
                None => return Ok(Ended::Closed),
            },
            // Un archivo del PC: cada uno en su tarea, para no frenar los mensajes.
            stream = conn.accept_uni() => {
                let mut stream = stream?;
                // Lo que sabe el PC ahora: puede haber prendido los archivos después de saludar.
                let has = |f: &str| ctx.live.lock_or_recover().as_ref().is_some_and(|(_, fs)| fs.iter().any(|x| x == f));
                match (&ctx.inbox, has(feature::FILES)) {
                    (Some(dir), true) => {
                        let receiving = Receiving {
                            dir: dir.clone(),
                            events: events.clone(),
                            outbox: tx.clone(),
                            offers_on: has(feature::FILE_OFFER),
                            limit: ctx.file_limit.load(Ordering::Relaxed),
                            offers: ctx.offers.clone(),
                            requested: ctx.requested.clone(),
                        };
                        tokio::spawn(async move {
                            if let Err(e) = receive_file(stream, receiving).await {
                                tracing::debug!("archivo del PC a medias: {e:#}");
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_solo_habilitado_y_compartiendo() {
        let event = InputEvent::Key { key: crate::PcKey::Home };
        assert!(screen_input_allowed(true, true, &event));
        assert!(!screen_input_allowed(false, true, &event));
        assert!(!screen_input_allowed(true, false, &event));
        assert!(!screen_input_allowed(true, true, &InputEvent::Text { text: String::new() }));
        assert!(!phone_features(false).iter().any(|f| f == feature::SCREEN_CONTROL));
        assert!(phone_features(true).iter().any(|f| f == feature::SCREEN_CONTROL));
    }

    #[test]
    fn el_control_es_del_pc_que_se_eligio() {
        assert!(!ScreenControl::default().allows("a"));
        let all = ScreenControl { enabled: true, only: None };
        assert!(all.allows("a") && all.allows("b"));
        let one = ScreenControl { enabled: true, only: Some("a".into()) };
        assert!(one.allows("a"));
        assert!(!one.allows("b"));
        // Apagado no vale para nadie, aunque quede un PC anotado.
        assert!(!ScreenControl { enabled: false, only: Some("a".into()) }.allows("a"));
    }
}
