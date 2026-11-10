//! El lado del escritorio: escucha, acepta solo equipos pareados y publica.

use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{atomic::{AtomicU64, Ordering}, Arc, Mutex},
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
    lock::MutexExt,
    proto::{
        feature, read_frame_max, write_frame, AgentCard, CallAction, CallState, ChangedFile, Choice, ClipItem, Decision, FileHeader,
        FileKind, InputEvent, PcCommand, PcState, PhoneNotice, ProjectInfo, ToDesktop, ToPhone, ALPN, MAX_CHANGED_FILES, MAX_CLIP_BYTES,
        MAX_GREETING, MAX_INPUT_TEXT, MAX_PROJECTS, MAX_PROJECT_SESSIONS,
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
/// Conexiones a la vez. El PC se anuncia en la LAN por mDNS: cualquiera de la
/// red puede abrir conexiones, y cada una ocupa una tarea hasta que saluda.
const MAX_CONNECTIONS: usize = 32;
/// Tope de una línea de [`Desktop::agent_output`] y del lote entero: un pegado
/// enorme en la consola no puede pasar el tope del mensaje y cortar la sesión.
const MAX_OUTPUT_LINE: usize = 8 * 1024;
const MAX_OUTPUT_BYTES: usize = 1 << 20;

pub struct DesktopConfig {
    pub secret_key: [u8; 32],
    /// Cómo se presenta en el celular ("PC de Carlos").
    pub name: String,
    /// Ids de los celulares ya pareados (lo que devolvió [`DesktopEvent::Paired`]).
    pub trusted: Vec<String>,
    /// Funciones prendidas desde el primer saludo (ver [`Desktop::enable`]):
    /// si se prenden después de abrir el canal, un celular que entra justo
    /// entremedio no las ve hasta reconectar.
    pub features: Vec<String>,
    /// Carpeta de los archivos que mande el celular.
    pub inbox: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DesktopEvent {
    /// Un equipo trajo un ticket válido. No entra hasta que alguien lo acepte
    /// con [`Desktop::approve_pairing`]: quien saque una foto al QR no se cuela.
    /// `install_id`: la instalación de la app ([`ToDesktop::Pair`]); con él quien hospeda
    /// sabe si es un celular que ya tenía (reinstalado) y lo reemplaza.
    PairRequested { device_id: String, device_name: String, install_id: Option<String> },
    /// Un equipo quedó pareado. Quien hospeda guarda `device_id`.
    Paired { device_id: String, device_name: String, install_id: Option<String> },
    /// `features`: lo que sabe hacer ese celular (ver [`feature`]).
    Connected { device_id: String, device_name: String, features: Vec<String>, install_id: Option<String> },
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
    /// Lo que pide o cuenta el celular con las funciones que este PC prendió con
    /// [`Desktop::enable`]. Sin prender ninguna, no llega nada de esto.
    Phone { device_id: String, request: PhoneRequest },
}

/// Lo nuevo que manda el celular, cada cosa detrás de su [`feature`].
#[derive(Debug, Clone, PartialEq)]
pub enum PhoneRequest {
    /// Una notificación nueva o actualizada ([`feature::NOTICES`]).
    Notice(PhoneNotice),
    NoticeRemoved { key: String },
    /// La llamada en curso; `None`: terminó ([`feature::CALLS`]).
    Call(Option<CallState>),
    /// Texto para un agente ([`feature::PROMPT`]). Las imágenes y los
    /// `clip_ids` (imágenes del historial de este PC) solo llegan con
    /// [`feature::AGENTS`] y ya revisadas ([`crate::prompt_images_ok`]).
    Prompt { agent_id: String, text: String, images: Vec<crate::PromptImage>, clip_ids: Vec<String> },
    /// Aceptar, abrir o detener un agente ([`feature::AGENTS`]; detener pide
    /// además [`feature::PROMPT`]).
    AgentAction { agent_id: String, action: crate::AgentAction },
    /// Mirar en vivo un agente; `None`: dejar de mirar ([`feature::WATCH`]). Quien
    /// hospeda contesta con [`Desktop::agent_output`].
    Watch { agent_id: Option<String> },
    /// Un archivo que ya quedó en la carpeta de entrada ([`feature::FILES`]).
    File { header: FileHeader, path: PathBuf },
    /// El puntero o el teclado de este PC ([`feature::REMOTE`]).
    Input(InputEvent),
    /// Abrir un enlace en el navegador del PC; ya viene revisado con
    /// [`crate::is_web_url`] ([`feature::LINKS`]).
    OpenUrl { url: String },
    /// La foto que se pidió con [`Desktop::take_photo`] ([`feature::PHOTO`]).
    Photo { mime: String, data: Vec<u8> },
    /// El celular pide prender estas funciones (solo [`feature::requestable`],
    /// sin repetir). Quien hospeda pregunta y contesta con
    /// [`Desktop::answer_access`] ([`feature::ACCESS`]).
    Access { features: Vec<String> },
    /// Los proyectos de Atic Code; se contesta con [`Desktop::send_projects`] ([`feature::PROJECTS`]).
    ListProjects,
    /// Una conversación nueva con su primer mensaje (`project_id` de
    /// [`ProjectInfo::id`]); se contesta con [`Desktop::chat_created`]. Llega solo con
    /// [`feature::PROJECTS`] y [`feature::PROMPT`] prendidos, y las imágenes revisadas.
    NewChat {
        req_id: String,
        project_id: String,
        agent: Option<String>,
        model: Option<String>,
        text: String,
        images: Vec<crate::PromptImage>,
    },
    /// El modelo (vacío: el predeterminado) y el esfuerzo de una conversación
    /// ([`feature::PROJECTS`] y [`feature::PROMPT`]). Si no se puede, [`Desktop::code_error`].
    SetModel { agent_id: String, model: String, effort: Option<String> },
    /// El modo de permisos de una conversación. Quien hospeda decide si permite
    /// `bypassPermissions` ([`feature::PROJECTS`] y [`feature::PROMPT`]).
    SetMode { agent_id: String, mode: String },
    /// Lo que cambió en el proyecto de un agente; se contesta con
    /// [`Desktop::send_changes`] ([`feature::PROJECTS`]).
    ListChanges { agent_id: String },
    /// Un archivo del proyecto que el celular bajó (o intentó bajar), para avisarlo en
    /// el PC. Lo revisa y lo manda este canal solo ([`feature::PROJECT_FILES`]).
    FileFetched { agent_id: String, rel_path: String, size: u64, error: Option<String> },
    /// Qué pasó con un enlace que se mandó con [`Desktop::open_url`]: se abrió en el
    /// celular o su usuario dijo que no ([`feature::LINK_RESULT`]). No llega siempre:
    /// un celular viejo no lo manda, y uno nuevo tampoco si nadie contestó.
    UrlResult { url: String, opened: bool },
}

/// Lo que se le mostró a un celular del proyecto de un agente: solo eso puede pedir.
#[derive(Debug, Clone, Default)]
struct Offered {
    root: PathBuf,
    paths: HashSet<String>,
}

/// Revisa un [`ToDesktop::FetchFile`]: la ruta tiene que estar en lo ofrecido a ese
/// celular para ese agente y pasar [`crate::project::resolve`].
fn check_fetch(
    offered: &HashMap<(EndpointId, String), Offered>,
    device: EndpointId,
    agent_id: &str,
    rel_path: &str,
) -> std::result::Result<crate::project::Resolved, crate::project::Refusal> {
    use crate::project::Refusal;
    let parts = crate::project::clean_parts(rel_path).ok_or(Refusal::BadPath)?;
    let list = offered.get(&(device, agent_id.to_string())).ok_or(Refusal::NotOffered)?;
    if !list.paths.contains(&parts.join("/")) {
        return Err(Refusal::NotOffered);
    }
    crate::project::resolve(&list.root, rel_path)
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
    /// Lo que este PC prendió ([`Desktop::enable`]); se anuncia al saludar.
    features: Mutex<Vec<String>>,
    /// Dónde quedan los archivos que manda el celular.
    inbox: Mutex<Option<PathBuf>>,
    /// Lo que sabe hacer cada celular conectado.
    phone_features: Mutex<HashMap<EndpointId, Vec<String>>>,
    /// Quién muestra el video de un celular ([`Desktop::on_video`]).
    video: Mutex<Option<Arc<crate::video::VideoHandler>>>,
    /// Quién comparte la pantalla del PC ([`Desktop::on_screen_request`]).
    screen: Mutex<Option<Arc<crate::video::ScreenHandler>>>,
    /// Los archivos de proyecto que cada celular puede pedir, por agente ([`check_fetch`]).
    offered: Mutex<HashMap<(EndpointId, String), Offered>>,
    /// Los archivos ofrecidos a un celular que esperan su respuesta ([`feature::FILE_OFFER`]).
    /// Por el id del envío, que es al azar: un celular no adivina el de otro.
    file_answers: crate::files::Answers,
    /// Tope de un archivo que manda el celular ([`Desktop::set_file_limit`]).
    file_limit: AtomicU64,
}

impl Shared {
    /// El token vale una sola vez y solo dentro de su plazo.
    fn take_pairing(&self, token: &str) -> bool {
        let mut slot = self.pairing.lock_or_recover();
        match slot.as_ref() {
            Some((t, until)) if t == token && Instant::now() < *until => {
                *slot = None;
                true
            }
            _ => false,
        }
    }

    fn is_trusted(&self, id: &EndpointId) -> bool {
        self.trusted.lock_or_recover().contains(id)
    }

    fn emit(&self, event: DesktopEvent) {
        let _ = self.events.send(event);
    }

    fn enabled(&self, f: &str) -> bool {
        self.features.lock_or_recover().iter().any(|x| x == f)
    }

    /// Los dos lados tienen la función: se puede mandar.
    fn both(&self, device: &EndpointId, f: &str) -> bool {
        self.enabled(f) && self.phone_features.lock_or_recover().get(device).is_some_and(|fs| fs.iter().any(|x| x == f))
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
            .alpns(vec![ALPN.to_vec(), crate::video::VIDEO_ALPN_2.to_vec(), crate::video::VIDEO_ALPN.to_vec()])
            // Para la pantalla del PC en el celular; los mensajes y archivos no pierden nada.
            .transport_config(crate::video::bbr_transport())
            .bind()
            .await
            .map_err(|e| anyhow!("no se pudo abrir el canal: {e:#}"))?;
        crate::add_mdns(&endpoint);

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
            // Lleva también la salida en vivo de un agente y las respuestas: con
            // poco espacio, un celular que se atrasa un momento las pierde.
            notices: broadcast::Sender::new(128),
            clips: broadcast::Sender::new(16),
            events,
            features: Mutex::new(config.features),
            inbox: Mutex::new(config.inbox),
            phone_features: Mutex::new(HashMap::new()),
            video: Mutex::new(None),
            screen: Mutex::new(None),
            offered: Mutex::new(HashMap::new()),
            file_answers: Default::default(),
            file_limit: AtomicU64::new(crate::proto::MAX_FILE_BYTES),
        });
        let accept_task = tokio::spawn(accept_loop(endpoint.clone(), shared.clone()));
        Ok((Self { endpoint, shared, accept_task }, rx))
    }

    pub fn id(&self) -> String {
        self.endpoint.id().to_string()
    }

    /// Prende funciones nuevas (ver [`feature`]): se anuncian al celular al
    /// saludar y desde ahí llegan sus [`DesktopEvent::Phone`]. `inbox` es la
    /// carpeta para los archivos que mande el celular (sin ella no se reciben).
    /// Vale para las sesiones que empiecen después.
    ///
    /// A los celulares conectados que lo entienden ([`feature::ACCESS`]) se les
    /// avisa al tiro: no hace falta reconectar para usar lo recién prendido.
    pub fn enable(&self, features: Vec<String>, inbox: Option<PathBuf>) {
        let changed = {
            let mut current = self.shared.features.lock_or_recover();
            let changed = *current != features;
            *current = features.clone();
            changed
        };
        *self.shared.inbox.lock_or_recover() = inbox;
        if !changed {
            return;
        }
        let phones: Vec<EndpointId> = self
            .shared
            .phone_features
            .lock_or_recover()
            .iter()
            .filter(|(_, fs)| fs.iter().any(|f| f == feature::ACCESS))
            .map(|(id, _)| *id)
            .collect();
        for phone in phones {
            let _ = self.shared.notices.send((Some(phone), ToPhone::Features { features: features.clone() }));
        }
    }

    /// La respuesta a [`PhoneRequest::Access`]. Si se aceptó, llamar antes a
    /// [`Desktop::enable`] con lo nuevo: así el celular ya lo tiene al leerla.
    pub fn answer_access(&self, device_id: &str, features: Vec<String>, granted: bool) -> bool {
        self.send_if(device_id, feature::ACCESS, ToPhone::AccessAnswer { features, granted })
    }

    /// Lo que sabe hacer un celular conectado. Vacío si no está o es viejo.
    pub fn phone_features(&self, device_id: &str) -> Vec<String> {
        let Ok(id) = device_id.parse::<EndpointId>() else {
            return Vec::new();
        };
        self.shared.phone_features.lock_or_recover().get(&id).cloned().unwrap_or_default()
    }

    /// Manda algo a un celular si los dos tienen la función `f`.
    fn send_if(&self, device_id: &str, f: &str, msg: ToPhone) -> bool {
        let Ok(to) = device_id.parse::<EndpointId>() else {
            return false;
        };
        self.shared.both(&to, f) && self.shared.notices.send((Some(to), msg)).is_ok()
    }

    /// Si el celular permite controlar su pantalla ahora.
    pub fn phone_screen_control(&self, device_id: &str) -> bool {
        self.shared.enabled(feature::SCREEN_CONTROL) && self.phone_features(device_id).iter().any(|f| f == feature::SCREEN_CONTROL)
    }

    /// Entrada al celular; nunca se manda a pares antiguos.
    pub fn phone_input(&self, device_id: &str, event: InputEvent) -> bool {
        event.valid() && self.send_if(device_id, feature::SCREEN_CONTROL, ToPhone::Input { event })
    }

    /// Contestar una notificación del celular.
    pub fn notice_reply(&self, device_id: &str, key: String, text: String) -> bool {
        self.send_if(device_id, feature::NOTICES, ToPhone::NoticeReply { key, text })
    }

    /// Descartar una notificación del celular.
    pub fn notice_dismiss(&self, device_id: &str, key: String) -> bool {
        self.send_if(device_id, feature::NOTICES, ToPhone::NoticeDismiss { key })
    }

    /// Contestar o colgar la llamada del celular.
    pub fn call_action(&self, device_id: &str, action: CallAction) -> bool {
        self.send_if(device_id, feature::CALLS, ToPhone::CallAction { action })
    }

    /// Hace sonar ese celular para encontrarlo; `false` para que se calle.
    pub fn ring_phone(&self, device_id: &str, on: bool) -> bool {
        self.send_if(device_id, feature::FIND, ToPhone::Ring { on })
    }

    /// Abre un enlace en el navegador de ese celular. Solo `http` y `https`
    /// ([`crate::is_web_url`]).
    pub fn open_url(&self, device_id: &str, url: String) -> bool {
        crate::is_web_url(&url) && self.send_if(device_id, feature::LINKS, ToPhone::OpenUrl { url })
    }

    /// Pide una foto con la cámara de ese celular; llega como [`PhoneRequest::Photo`].
    pub fn take_photo(&self, device_id: &str) -> bool {
        self.send_if(device_id, feature::PHOTO, ToPhone::TakePhoto)
    }

    /// Le pide al celular que comparta su pantalla ([`feature::SCREEN_REQUEST`]); con
    /// `open`, la clave de un aviso suyo, también abre la app de ese aviso. El usuario
    /// del celular tiene que aceptar: si no acepta, no pasa nada.
    pub fn request_screen(&self, device_id: &str, open: Option<String>) -> bool {
        self.send_if(device_id, feature::SCREEN_REQUEST, ToPhone::ShareScreen { open })
    }

    /// Le pide al celular que deje controlarlo ([`feature::CONTROL_REQUEST`]). Decide el
    /// usuario del celular; si acepta, llega [`DesktopEvent`] con las funciones nuevas.
    pub fn request_control(&self, device_id: &str) -> bool {
        self.send_if(device_id, feature::CONTROL_REQUEST, ToPhone::RequestControl)
    }

    /// Lo que escribió el agente que ese celular mira (ver [`PhoneRequest::Watch`]).
    pub fn agent_output(&self, device_id: &str, agent_id: String, lines: Vec<String>, reset: bool) -> bool {
        self.send_if(device_id, feature::WATCH, ToPhone::AgentOutput { agent_id, lines: cap_output(lines), reset })
    }

    /// Lo que escribió el agente que ese celular mira, por partes y con
    /// imágenes. Solo a un celular con [`feature::AGENTS`]; si no, `false` y va
    /// por [`Desktop::agent_output`]. Lo que no cabe en un mensaje se corta:
    /// textos larguísimos y, de atrás hacia adelante, imágenes y partes viejas.
    pub fn agent_entries(&self, device_id: &str, agent_id: String, entries: Vec<crate::AgentEntry>, reset: bool) -> bool {
        let Ok(to) = device_id.parse::<EndpointId>() else {
            return false;
        };
        let has = self.shared.phone_features.lock_or_recover().get(&to).is_some_and(|fs| fs.iter().any(|f| f == feature::AGENTS));
        let entries = tools_for(entries, self.shared.both(&to, feature::TOOLS));
        has && self.shared.enabled(feature::WATCH)
            && self.shared.notices.send((Some(to), ToPhone::AgentEntries { agent_id, entries: cap_entries(entries), reset })).is_ok()
    }

    /// Contesta [`PhoneRequest::ListProjects`]. Lo que pasa de los topes se omite.
    pub fn send_projects(
        &self,
        device_id: &str,
        mut projects: Vec<ProjectInfo>,
        agents: Vec<Choice>,
        models: Vec<Choice>,
        efforts: Vec<Choice>,
        modes: Vec<Choice>,
        bypass: bool,
    ) -> bool {
        projects.truncate(MAX_PROJECTS);
        for project in &mut projects {
            project.sessions.truncate(MAX_PROJECT_SESSIONS);
        }
        self.send_if(device_id, feature::PROJECTS, ToPhone::Projects { projects, agents, models, efforts, modes, bypass })
    }

    /// Contesta [`PhoneRequest::NewChat`]: la clave de la conversación nueva o por qué no.
    pub fn chat_created(&self, device_id: &str, req_id: String, result: std::result::Result<String, String>) -> bool {
        let msg = match result {
            Ok(key) => ToPhone::ChatCreated { req_id, ok: true, reason: None, chat_key: Some(key) },
            Err(reason) => ToPhone::ChatCreated { req_id, ok: false, reason: Some(reason), chat_key: None },
        };
        self.send_if(device_id, feature::PROJECTS, msg)
    }

    /// Avisa que lo que pidió el celular sobre un agente no se pudo hacer.
    pub fn code_error(&self, device_id: &str, agent_id: Option<String>, reason: String) -> bool {
        self.send_if(device_id, feature::PROJECTS, ToPhone::CodeError { agent_id, reason })
    }

    /// Contesta [`PhoneRequest::ListChanges`]. `root` es la carpeta del proyecto del
    /// agente: desde ahora ese celular puede pedir esos archivos (salvo los bloqueados
    /// y los borrados) con [`ToDesktop::FetchFile`], y nada más de ese agente.
    pub fn send_changes(&self, device_id: &str, agent_id: String, root: &Path, branch: Option<String>, mut files: Vec<ChangedFile>) -> bool {
        let Ok(to) = device_id.parse::<EndpointId>() else {
            return false;
        };
        files.truncate(MAX_CHANGED_FILES);
        for file in &mut files {
            file.blocked |= crate::project::is_blocked(&file.rel_path);
        }
        let paths = files
            .iter()
            .filter(|f| !f.blocked && f.status != "D")
            .filter_map(|f| crate::project::clean_parts(&f.rel_path).map(|p| p.join("/")))
            .collect();
        self.shared.offered.lock_or_recover().insert((to, agent_id.clone()), Offered { root: root.to_path_buf(), paths });
        let root_name = root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        self.send_if(device_id, feature::PROJECTS, ToPhone::Changes { agent_id, root_name, branch, files })
    }

    /// Avisa a los celulares que bajan archivos de proyecto ([`feature::PROJECT_FILES`])
    /// que un turno dejó un artefacto, y se lo deja pedir. Devuelve a cuántos salió.
    pub fn artifact_ready(&self, agent_id: &str, root: &Path, rel_path: &str, size: u64) -> usize {
        let Some(parts) = crate::project::clean_parts(rel_path) else {
            return 0;
        };
        if crate::project::is_blocked(rel_path) || !self.shared.enabled(feature::PROJECT_FILES) {
            return 0;
        }
        let rel = parts.join("/");
        let phones: Vec<EndpointId> = self
            .shared
            .phone_features
            .lock_or_recover()
            .iter()
            .filter(|(_, fs)| fs.iter().any(|f| f == feature::PROJECT_FILES))
            .map(|(id, _)| *id)
            .collect();
        let mut sent = 0;
        for phone in phones {
            {
                let mut offered = self.shared.offered.lock_or_recover();
                let list = offered.entry((phone, agent_id.to_string())).or_default();
                if list.root != root {
                    *list = Offered { root: root.to_path_buf(), paths: HashSet::new() };
                }
                list.paths.insert(rel.clone());
            }
            let msg = ToPhone::ArtifactReady { agent_id: agent_id.to_string(), rel_path: rel.clone(), size, mime: crate::project::mime_for(&rel).to_string() };
            if self.shared.notices.send((Some(phone), msg)).is_ok() {
                sent += 1;
            }
        }
        sent
    }

    /// Manda un archivo al celular por su propia corriente. Termina cuando el
    /// celular lo tiene entero.
    ///
    /// Con [`feature::FILE_OFFER`] en los dos lados, primero se ofrece: el celular
    /// decide con el nombre y el tamaño (su usuario, si no lo había pedido) y recién
    /// entonces viaja el contenido. Si no lo acepta, pesa demasiado o nadie contesta,
    /// el error lleva un [`crate::files::SendRefused`]
    /// (`error.downcast_ref::<SendRefused>()`) y no se mandó nada.
    pub async fn send_file(&self, device_id: &str, path: &Path, mime: String, kind: FileKind, meta: Option<String>) -> Result<()> {
        let to: EndpointId = device_id.parse().context("id de equipo inválido")?;
        if !self.shared.both(&to, feature::FILES) {
            bail!("el celular no recibe archivos");
        }
        let conn = self.shared.live.lock_or_recover().get(&to).cloned().context("el celular no está conectado")?;
        let size = tokio::fs::metadata(path).await?.len();
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let header = FileHeader { id: new_token(), name, mime, size, kind, meta, offer: false };
        let answers = self.shared.both(&to, feature::FILE_OFFER).then(|| self.shared.file_answers.clone());
        send_to_phone(&conn, header, path, answers).await
    }

    /// Tope de un archivo que manda el celular, en bytes (nunca más que
    /// [`crate::MAX_FILE_BYTES`]). Se revisa con el encabezado, antes de escribir nada.
    pub fn set_file_limit(&self, bytes: u64) {
        self.shared.file_limit.store(bytes.min(crate::proto::MAX_FILE_BYTES), Ordering::Relaxed);
    }

    /// Abre una ventana de pareo nueva (la anterior deja de valer) y devuelve
    /// el texto del QR. Espera unos segundos a tener relay para que el ticket
    /// sirva también fuera de la LAN.
    pub async fn pairing_ticket(&self) -> String {
        let _ = tokio::time::timeout(Duration::from_secs(5), self.endpoint.online()).await;
        let token = new_token();
        *self.shared.pairing.lock_or_recover() = Some((token.clone(), Instant::now() + PAIRING_TTL));
        PairingTicket {
            addr: self.endpoint.addr(),
            token,
            desktop_name: self.shared.name.clone(),
        }
        .encode()
    }

    /// Cierra la ventana de pareo sin esperar a que venza.
    pub fn cancel_pairing(&self) {
        *self.shared.pairing.lock_or_recover() = None;
    }

    /// Quién muestra el video que manda un celular ([`crate::video`]): recibe cada
    /// pedido y decide. Sin quién, se rechaza.
    pub fn on_video(&self, handler: impl Fn(crate::video::VideoLink) -> std::result::Result<(), String> + Send + Sync + 'static) {
        *self.shared.video.lock_or_recover() = Some(Arc::new(handler));
    }

    /// Quién comparte la pantalla del PC con un celular que la pide ([`crate::video`]):
    /// recibe cada pedido y entrega los cuadros. Sin quién, se rechaza.
    pub fn on_screen_request(
        &self,
        handler: impl Fn(crate::video::ScreenRequest) -> std::result::Result<crate::video::ScreenSource, String> + Send + Sync + 'static,
    ) {
        *self.shared.screen.lock_or_recover() = Some(Arc::new(handler));
    }

    /// Acepta o rechaza un [`DesktopEvent::PairRequested`]. `false` si ese
    /// pedido ya no existe (venció o el celular se fue).
    pub fn approve_pairing(&self, device_id: &str, accept: bool) -> bool {
        let Ok(id) = device_id.parse::<EndpointId>() else {
            return false;
        };
        match self.shared.approvals.lock_or_recover().remove(&id) {
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
        self.shared.trusted.lock_or_recover().remove(&id);
        let live = self.shared.live.lock_or_recover().remove(&id);
        self.shared.phone_features.lock_or_recover().remove(&id);
        self.shared.offered.lock_or_recover().retain(|(d, _), _| *d != id);
        if let Some(conn) = live {
            conn.close(CLOSE_REVOKED.into(), b"revoked");
            // La sesión ya no es la vigente y no lo avisaría: quien hospeda
            // lo seguiría viendo conectado (y mirando su agente en vivo).
            self.shared.emit(DesktopEvent::Disconnected { device_id: device_id.to_string() });
        }
        Ok(())
    }

    pub async fn shutdown(self) {
        self.close().await;
    }

    /// Corta todo avisando: el celular ve «el PC cerró la conexión» al
    /// instante, en vez de esperar a que la conexión venza sola.
    pub async fn close(&self) {
        self.accept_task.abort();
        self.endpoint.close().await;
    }
}

async fn accept_loop(endpoint: Endpoint, shared: Arc<Shared>) {
    let slots = Arc::new(tokio::sync::Semaphore::new(MAX_CONNECTIONS));
    while let Some(incoming) = endpoint.accept().await {
        let Ok(slot) = slots.clone().try_acquire_owned() else {
            debug!("demasiadas conexiones; se rechaza una");
            incoming.refuse();
            continue;
        };
        let shared = shared.clone();
        tokio::spawn(async move {
            let _slot = slot;
            let conn = match incoming.accept() {
                Ok(accepting) => match accepting.await {
                    Ok(conn) => conn,
                    Err(e) => return debug!("conexión fallida: {e:#}"),
                },
                Err(e) => return debug!("conexión rechazada: {e:#}"),
            };
            let device = conn.remote_id();
            // El video va por su propia conexión (`atic/video/1` o `/2`).
            if crate::video::is_video_alpn(conn.alpn()) {
                let allowed = |f: &str, off: &str| {
                    if !shared.is_trusted(&device) {
                        Err("Este celular no está pareado.".to_string())
                    } else if !shared.enabled(f) {
                        Err(off.to_string())
                    } else {
                        Ok(())
                    }
                };
                let not_ready = || "Atic no está listo para el video ahora.".to_string();
                let show = shared.video.lock_or_recover().clone();
                let share = shared.screen.lock_or_recover().clone();
                let video = crate::video::DesktopVideo {
                    show: allowed(feature::SCREEN, "La pantalla del celular está apagada en Ajustes de Atic.").and_then(|()| show.ok_or_else(not_ready)),
                    share: allowed(feature::SCREEN_OUT, "Compartir la pantalla del PC está apagado en Ajustes de Atic.").and_then(|()| share.ok_or_else(not_ready)),
                };
                if let Err(e) = crate::video::serve_desktop(conn, video).await {
                    debug!("video de {device} terminó: {e:#}");
                }
                return;
            }
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
    let first: ToDesktop = tokio::time::timeout(HELLO_TIMEOUT, read_frame_max(&mut recv, MAX_GREETING))
        .await
        .context("no saludó a tiempo")??
        .context("cerró sin saludar")?;

    let (device_name, phone_features, install_id) = match first {
        // Si la conexión se cortó justo después de aceptar, el celular vuelve con
        // el ticket ya gastado: ya es de confianza, así que entra como un saludo.
        ToDesktop::Pair { device_name, features, install_id, .. } if shared.is_trusted(&device) => (device_name, features, install_id),
        ToDesktop::Pair { token, device_name, features, install_id } => {
            if !shared.take_pairing(&token) {
                let reason = "El código de vinculación venció o ya se usó.";
                return reject(&conn, &mut send, reason).await;
            }
            write_frame(&mut send, &ToPhone::AwaitingApproval).await?;
            if !await_approval(shared, device, &device_name, install_id.clone()).await {
                let reason = "El PC no aceptó la vinculación.";
                return reject(&conn, &mut send, reason).await;
            }
            shared.trusted.lock_or_recover().insert(device);
            shared.emit(DesktopEvent::Paired {
                device_id: device.to_string(),
                device_name: device_name.clone(),
                install_id: install_id.clone(),
            });
            (device_name, features, install_id)
        }
        ToDesktop::Hello { device_name, features, install_id } => {
            if !shared.is_trusted(&device) {
                let reason = "Este celular no está vinculado con el PC.";
                return reject(&conn, &mut send, reason).await;
            }
            (device_name, features, install_id)
        }
        other => bail!("primer mensaje inesperado: {other:?}"),
    };

    let features = shared.features.lock_or_recover().clone();
    write_frame(&mut send, &ToPhone::Welcome { desktop_name: shared.name.clone(), features }).await?;
    shared.phone_features.lock_or_recover().insert(device, phone_features.clone());
    // Una conexión nueva del mismo equipo reemplaza a la anterior.
    if let Some(old) = shared.live.lock_or_recover().insert(device, conn.clone()) {
        old.close(0u32.into(), b"replaced");
    }
    shared.emit(DesktopEvent::Connected {
        device_id: device.to_string(),
        device_name,
        features: phone_features,
        install_id,
    });
    let result = session(&mut send, recv, &conn, device, shared).await;
    // Si otra conexión del mismo equipo la reemplazó, el celular sigue
    // conectado: avisar que se fue lo sacaba de la lista de quien hospeda.
    let current = {
        let mut live = shared.live.lock_or_recover();
        let current = live.get(&device).is_some_and(|c| c.stable_id() == conn.stable_id());
        if current {
            live.remove(&device);
            shared.phone_features.lock_or_recover().remove(&device);
            shared.offered.lock_or_recover().retain(|(d, _), _| *d != device);
        }
        current
    };
    if current {
        shared.emit(DesktopEvent::Disconnected { device_id: device.to_string() });
    }
    result
}

/// Manda un archivo a un celular; con `answers` (los dos lados tienen
/// [`feature::FILE_OFFER`]), ofreciéndolo primero.
async fn send_to_phone(conn: &Connection, header: FileHeader, path: &Path, answers: Option<crate::files::Answers>) -> Result<()> {
    match answers {
        Some(answers) => crate::files::send_offered(conn, header, path, &answers, |_, _| {}).await,
        None => crate::files::send(conn, header, path, |_, _| {}).await,
    }
}

/// Recibe un archivo del celular en `dir`. Uno ofrecido ([`FileHeader::offer`], con
/// `offers`: los dos lados tienen [`feature::FILE_OFFER`]) se contesta con `answer`
/// antes de leer su contenido: se acepta salvo que pase el tope (`limit`). Preguntarle
/// a quien está en el PC queda para quien hospeda; acá se decide solo por el tamaño.
async fn receive_from_phone(mut stream: RecvStream, dir: &Path, offers: bool, limit: u64, answer: impl Fn(ToPhone)) -> Result<(FileHeader, PathBuf)> {
    let header = crate::files::read_header(&mut stream).await?;
    let too_large = header.size > limit;
    if header.offer && offers {
        if header.id.is_empty() || header.id.len() > MAX_FILE_ID {
            crate::files::refuse(&mut stream, false);
            bail!("archivo ofrecido con un id fuera de tope");
        }
        answer(ToPhone::FileAnswer { id: header.id.clone(), accept: !too_large, too_large });
    }
    if too_large {
        crate::files::refuse(&mut stream, true);
        bail!("el archivo pasa el tope");
    }
    let path = crate::files::receive_body(stream, &header, dir, |_, _| {}).await?;
    Ok((header, path))
}

/// Tope del id de un archivo ofrecido: vuelve en la respuesta.
const MAX_FILE_ID: usize = 128;

/// Recorta la salida de un agente para que quepa en un mensaje: cada línea a
/// [`MAX_OUTPUT_LINE`] y, si el lote pasa [`MAX_OUTPUT_BYTES`], quedan las últimas.
fn cap_output(lines: Vec<String>) -> Vec<String> {
    let mut lines: Vec<String> = lines
        .into_iter()
        .map(|mut l| {
            if l.len() > MAX_OUTPUT_LINE {
                let mut end = MAX_OUTPUT_LINE;
                while !l.is_char_boundary(end) {
                    end -= 1;
                }
                l.truncate(end);
                l.push('…');
            }
            l
        })
        .collect();
    let mut total = 0;
    let keep = lines.iter().rev().take_while(|l| {
        total += l.len() + 8;
        total <= MAX_OUTPUT_BYTES
    }).count();
    lines.drain(..lines.len() - keep);
    lines
}

/// Lo que entra en un [`ToPhone::AgentEntries`]: cada texto hasta
/// `MAX_ENTRY_TEXT`; de las más nuevas a las viejas, hasta `MAX_OUTPUT_BYTES` de
/// texto e imágenes hasta `MAX_ENTRY_IMAGES` (base64 incluido cabe en el frame).
fn cap_entries(entries: Vec<crate::AgentEntry>) -> Vec<crate::AgentEntry> {
    let (mut text, mut images) = (0, 0);
    let mut kept: Vec<crate::AgentEntry> = Vec::new();
    for mut entry in entries.into_iter().rev() {
        if entry.text.len() > MAX_ENTRY_TEXT {
            let mut end = MAX_ENTRY_TEXT;
            while !entry.text.is_char_boundary(end) {
                end -= 1;
            }
            entry.text.truncate(end);
            entry.text.push('…');
        }
        let tool = entry.tool.as_ref().map_or(0, |t| {
            t.verb.len() + t.target.len() + t.id.len() + t.detail.as_ref().map_or(0, String::len) + t.diff.as_ref().map_or(0, String::len) + 64
        });
        text += entry.text.len() + tool + 16;
        if text > MAX_OUTPUT_BYTES {
            break;
        }
        entry.images.retain(|image| {
            images += image.data.len();
            images <= MAX_ENTRY_IMAGES
        });
        kept.push(entry);
    }
    kept.reverse();
    kept
}

/// Las herramientas con detalle, a su tope, a un celular que las entiende; a uno
/// viejo, sin ellas y sin las entradas que solo traen un resultado.
fn tools_for(entries: Vec<crate::AgentEntry>, tools: bool) -> Vec<crate::AgentEntry> {
    entries
        .into_iter()
        .filter(|e| tools || e.tool.is_none() || !e.text.is_empty())
        .map(|mut e| {
            match (&mut e.tool, tools) {
                (Some(tool), true) => tool.cap(),
                (_, false) => e.tool = None,
                _ => {}
            }
            e
        })
        .collect()
}

const MAX_ENTRY_TEXT: usize = 32 * 1024;
const MAX_ENTRY_IMAGES: usize = 4 << 20;

/// Pide la aprobación de quien hospeda y espera la respuesta (o el plazo).
async fn await_approval(shared: &Shared, device: EndpointId, device_name: &str, install_id: Option<String>) -> bool {
    let (tx, rx) = oneshot::channel();
    shared.approvals.lock_or_recover().insert(device, tx);
    shared.emit(DesktopEvent::PairRequested {
        device_id: device.to_string(),
        device_name: device_name.to_string(),
        install_id,
    });
    let accepted = matches!(tokio::time::timeout(APPROVAL_TIMEOUT, rx).await, Ok(Ok(true)));
    shared.approvals.lock_or_recover().remove(&device);
    accepted
}

/// Avisa el motivo y espera a que el otro lado cierre: si la conexión se
/// suelta enseguida, QUIC la corta antes de entregar el mensaje y el celular
/// lo vería como un corte de red (y reintentaría).
async fn reject(conn: &Connection, send: &mut SendStream, reason: &str) -> Result<()> {
    // Sin esto no hay cómo saber después por qué un celular dejó de conectarse.
    tracing::warn!(equipo = %conn.remote_id(), "celular rechazado: {reason}");
    write_frame(send, &ToPhone::Rejected { reason: reason.to_string() }).await?;
    let _ = send.finish();
    let _ = tokio::time::timeout(Duration::from_secs(3), conn.closed()).await;
    Ok(())
}

async fn session(
    send: &mut SendStream,
    recv: RecvStream,
    conn: &Connection,
    device: EndpointId,
    shared: &Shared,
) -> Result<()> {
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
                Ok((to, msg)) if to.is_none_or(|t| t == device) => {
                    if !matches!(&msg, ToPhone::Input { .. }) || shared.both(&device, feature::SCREEN_CONTROL) {
                        write_frame(send, &msg).await?;
                    }
                },
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
            // Un archivo del celular: cada uno en su tarea, para no frenar los mensajes.
            stream = conn.accept_uni() => {
                let Ok(stream) = stream else { return Ok(()) };
                let inbox = shared.inbox.lock_or_recover().clone();
                match inbox {
                    Some(dir) if shared.enabled(feature::FILES) && shared.is_trusted(&device) => {
                        let events = shared.events.clone();
                        let offers = shared.both(&device, feature::FILE_OFFER);
                        let limit = shared.file_limit.load(Ordering::Relaxed);
                        let notices = shared.notices.clone();
                        tokio::spawn(async move {
                            let answer = |msg| {
                                let _ = notices.send((Some(device), msg));
                            };
                            match receive_from_phone(stream, &dir, offers, limit, answer).await {
                                Ok((header, path)) => {
                                    let _ = events.send(DesktopEvent::Phone {
                                        device_id: device.to_string(),
                                        request: PhoneRequest::File { header, path },
                                    });
                                }
                                Err(e) => debug!("archivo de {device} a medias: {e:#}"),
                            }
                        });
                    }
                    _ => {
                        let mut stream = stream;
                        let _ = stream.stop(0u32.into());
                    }
                }
            }
            msg = incoming.recv() => match msg.unwrap_or(Ok(None))? {
                None => return Ok(()),
                // Un equipo revocado con la sesión abierta ya no decide ni copia nada.
                Some(_) if !shared.is_trusted(&device) => {}
                Some(ToDesktop::Features { features }) if shared.enabled(feature::SCREEN_CONTROL) => {
                    shared.phone_features.lock_or_recover().insert(device, features);
                }
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
                    if command.feature().is_none_or(|f| shared.enabled(f)) {
                        shared.emit(DesktopEvent::Command { device_id: device.to_string(), command });
                    }
                }
                Some(ToDesktop::Notice { notice }) if shared.enabled(feature::NOTICES) => {
                    shared.emit(DesktopEvent::Phone { device_id: device.to_string(), request: PhoneRequest::Notice(notice) });
                }
                Some(ToDesktop::NoticeRemoved { key }) if shared.enabled(feature::NOTICES) => {
                    shared.emit(DesktopEvent::Phone { device_id: device.to_string(), request: PhoneRequest::NoticeRemoved { key } });
                }
                Some(ToDesktop::Call { call }) if shared.enabled(feature::CALLS) => {
                    shared.emit(DesktopEvent::Phone { device_id: device.to_string(), request: PhoneRequest::Call(call) });
                }
                Some(ToDesktop::Prompt { agent_id, text, mut images, mut clip_ids }) if shared.enabled(feature::PROMPT) => {
                    // Sin `agents` (o pasadas de tope) las imágenes no van: queda el texto.
                    if !shared.enabled(feature::AGENTS) || !crate::prompt_images_ok(&images) {
                        images.clear();
                    }
                    clip_ids.retain(|id| !id.is_empty() && id.len() <= 128);
                    if !shared.enabled(feature::AGENTS) || clip_ids.len() > crate::MAX_PROMPT_IMAGES {
                        clip_ids.clear();
                    }
                    if !text.trim().is_empty() || !images.is_empty() || !clip_ids.is_empty() {
                        let request = PhoneRequest::Prompt { agent_id, text, images, clip_ids };
                        shared.emit(DesktopEvent::Phone { device_id: device.to_string(), request });
                    }
                }
                Some(ToDesktop::AgentAction { agent_id, action }) if shared.enabled(feature::AGENTS) => {
                    if action != crate::AgentAction::Stop || shared.enabled(feature::PROMPT) {
                        shared.emit(DesktopEvent::Phone { device_id: device.to_string(), request: PhoneRequest::AgentAction { agent_id, action } });
                    }
                }
                Some(ToDesktop::Watch { agent_id }) if shared.enabled(feature::WATCH) => {
                    shared.emit(DesktopEvent::Phone { device_id: device.to_string(), request: PhoneRequest::Watch { agent_id } });
                }
                Some(ToDesktop::Input { event }) if shared.enabled(feature::REMOTE) => {
                    if !matches!(&event, InputEvent::Text { text } if text.is_empty() || text.len() > MAX_INPUT_TEXT) {
                        shared.emit(DesktopEvent::Phone { device_id: device.to_string(), request: PhoneRequest::Input(event) });
                    }
                }
                Some(ToDesktop::OpenUrl { url }) if shared.enabled(feature::LINKS) => {
                    if crate::is_web_url(&url) {
                        shared.emit(DesktopEvent::Phone { device_id: device.to_string(), request: PhoneRequest::OpenUrl { url } });
                    }
                }
                Some(ToDesktop::RequestAccess { features }) if shared.enabled(feature::ACCESS) => {
                    let mut wanted: Vec<String> = Vec::new();
                    for f in features.into_iter().take(8) {
                        if feature::requestable(&f) && !wanted.contains(&f) {
                            wanted.push(f);
                        }
                    }
                    if !wanted.is_empty() {
                        shared.emit(DesktopEvent::Phone { device_id: device.to_string(), request: PhoneRequest::Access { features: wanted } });
                    }
                }
                Some(ToDesktop::Photo { mime, data }) if shared.enabled(feature::PHOTO) => {
                    if !data.is_empty() && data.len() <= crate::proto::MAX_IMAGE_BYTES {
                        shared.emit(DesktopEvent::Phone { device_id: device.to_string(), request: PhoneRequest::Photo { mime, data } });
                    }
                }
                Some(ToDesktop::ListProjects) if shared.enabled(feature::PROJECTS) => {
                    shared.emit(DesktopEvent::Phone { device_id: device.to_string(), request: PhoneRequest::ListProjects });
                }
                Some(ToDesktop::NewChat { req_id, project_id, agent, model, text, mut images })
                    if shared.enabled(feature::PROJECTS) && shared.enabled(feature::PROMPT) =>
                {
                    let short = |s: &str, max: usize| !s.is_empty() && s.len() <= max;
                    if !crate::prompt_images_ok(&images) {
                        images.clear();
                    }
                    let fits = short(&req_id, 128)
                        && short(&project_id, 64)
                        && agent.as_deref().is_none_or(|a| short(a, 64))
                        && model.as_deref().is_none_or(|m| m.len() <= 200);
                    if !fits {
                        debug!("chat nuevo con datos fuera de tope de {device}");
                    } else if text.trim().is_empty() && images.is_empty() {
                        let reason = "Escribe el primer mensaje del chat.".to_string();
                        write_frame(send, &ToPhone::ChatCreated { req_id, ok: false, reason: Some(reason), chat_key: None }).await?;
                    } else {
                        let request = PhoneRequest::NewChat { req_id, project_id, agent, model, text, images };
                        shared.emit(DesktopEvent::Phone { device_id: device.to_string(), request });
                    }
                }
                Some(ToDesktop::SetModel { agent_id, model, effort })
                    if shared.enabled(feature::PROJECTS) && shared.enabled(feature::PROMPT) =>
                {
                    if model.len() <= 200 && effort.as_deref().is_none_or(|e| e.len() <= 32) {
                        shared.emit(DesktopEvent::Phone { device_id: device.to_string(), request: PhoneRequest::SetModel { agent_id, model, effort } });
                    }
                }
                Some(ToDesktop::SetMode { agent_id, mode }) if shared.enabled(feature::PROJECTS) && shared.enabled(feature::PROMPT) => {
                    if !mode.is_empty() && mode.len() <= 64 {
                        shared.emit(DesktopEvent::Phone { device_id: device.to_string(), request: PhoneRequest::SetMode { agent_id, mode } });
                    }
                }
                Some(ToDesktop::ListChanges { agent_id }) if shared.enabled(feature::PROJECTS) => {
                    shared.emit(DesktopEvent::Phone { device_id: device.to_string(), request: PhoneRequest::ListChanges { agent_id } });
                }
                Some(ToDesktop::FetchFile { agent_id, rel_path, req_id }) if shared.enabled(feature::PROJECT_FILES) => {
                    if req_id.is_empty() || req_id.len() > 128 {
                        continue;
                    }
                    let checked = if shared.both(&device, feature::FILES) {
                        check_fetch(&shared.offered.lock_or_recover(), device, &agent_id, &rel_path).map_err(|r| r.reason().to_string())
                    } else {
                        Err("El celular no recibe archivos.".to_string())
                    };
                    match checked {
                        Err(reason) => {
                            tracing::info!(equipo = %device, %rel_path, %reason, "celular: archivo de proyecto rechazado");
                            write_frame(send, &ToPhone::FetchResult { req_id, ok: false, reason: Some(reason), size: 0, name: String::new(), mime: String::new() }).await?;
                        }
                        Ok(file) => {
                            let name = file.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                            let mime = crate::project::mime_for(&name).to_string();
                            write_frame(send, &ToPhone::FetchResult { req_id: req_id.clone(), ok: true, reason: None, size: file.size, name: name.clone(), mime: mime.clone() }).await?;
                            let conn = conn.clone();
                            let events = shared.events.clone();
                            let answers = shared.both(&device, feature::FILE_OFFER).then(|| shared.file_answers.clone());
                            tokio::spawn(async move {
                                let meta = serde_json::json!({ "agentId": agent_id, "relPath": rel_path }).to_string();
                                let header = FileHeader { id: req_id, name, mime, size: file.size, kind: FileKind::File, meta: Some(meta), offer: false };
                                let sent = send_to_phone(&conn, header, &file.path, answers).await;
                                let error = sent.err().map(|e| format!("{e:#}"));
                                let request = PhoneRequest::FileFetched { agent_id, rel_path, size: file.size, error };
                                let _ = events.send(DesktopEvent::Phone { device_id: device.to_string(), request });
                            });
                        }
                    }
                }
                Some(ToDesktop::FileAnswer { id, accept, too_large }) if shared.enabled(feature::FILE_OFFER) => {
                    shared.file_answers.deliver(&id, crate::files::FileAnswer { accept, too_large });
                }
                Some(ToDesktop::UrlResult { url, opened }) if shared.enabled(feature::LINK_RESULT) => {
                    if crate::is_web_url(&url) {
                        shared.emit(DesktopEvent::Phone { device_id: device.to_string(), request: PhoneRequest::UrlResult { url, opened } });
                    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_salida_de_un_agente_cabe_en_un_mensaje() {
        let long = "á".repeat(MAX_OUTPUT_LINE);
        let out = cap_output(vec![long, "ok".into()]);
        assert!(out[0].len() <= MAX_OUTPUT_LINE + '…'.len_utf8());
        assert_eq!(out[1], "ok");
        // Un lote gigante deja las últimas líneas, que son las que se miran.
        let many: Vec<String> = (0..1000).map(|i| format!("{i:0>4}{}", "x".repeat(4000))).collect();
        let out = cap_output(many);
        assert!(out.iter().map(|l| l.len()).sum::<usize>() <= MAX_OUTPUT_BYTES);
        assert!(out.last().unwrap().starts_with("0999"));
    }

    fn scratch(what: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("atic-fetch-{what}-{}", new_token()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn offer(root: &Path, device: EndpointId, paths: &[&str]) -> HashMap<(EndpointId, String), Offered> {
        let mut offered = HashMap::new();
        offered.insert((device, "a".to_string()), Offered { root: root.to_path_buf(), paths: paths.iter().map(|p| p.to_string()).collect() });
        offered
    }

    #[test]
    fn solo_se_baja_lo_ofrecido_y_dentro_del_proyecto() {
        use crate::project::Refusal;
        let root = scratch("raiz");
        let outside = scratch("afuera");
        std::fs::create_dir_all(root.join("app/build/outputs")).unwrap();
        std::fs::write(root.join("app/build/outputs/app.apk"), b"apk").unwrap();
        std::fs::write(root.join(".env"), b"CLAVE=1").unwrap();
        std::fs::write(root.join("nota.txt"), b"hola").unwrap();
        std::fs::write(outside.join("ajeno.txt"), b"ajeno").unwrap();
        std::fs::create_dir_all(root.join("carpeta")).unwrap();
        let device = SecretKey::generate().public();
        let other = SecretKey::generate().public();
        let offered = offer(&root, device, &["app/build/outputs/app.apk", ".env", "nota.txt", "borrado.txt", "carpeta", "../afuera/ajeno.txt"]);

        let ok = check_fetch(&offered, device, "a", "app/build/outputs/app.apk").unwrap();
        assert_eq!((ok.size, std::fs::read(&ok.path).unwrap()), (3, b"apk".to_vec()));
        // Con barras de Windows es la misma ruta.
        assert!(check_fetch(&offered, device, "a", r"app\build\outputs\app.apk").is_ok());
        // Traversal, absolutas, unidades y UNC: ni se miran.
        for bad in ["../afuera/ajeno.txt", "nota.txt/../../x", "/etc/passwd", r"C:\Windows\win.ini", r"\\pc\c$\x"] {
            assert_eq!(check_fetch(&offered, device, "a", bad), Err(Refusal::BadPath), "{bad}");
        }
        // La lista de bloqueo gana aunque se haya ofrecido.
        assert_eq!(check_fetch(&offered, device, "a", ".env"), Err(Refusal::Blocked));
        // Lo que ya no está, o no es un archivo.
        assert_eq!(check_fetch(&offered, device, "a", "borrado.txt"), Err(Refusal::NotFound));
        assert_eq!(check_fetch(&offered, device, "a", "carpeta"), Err(Refusal::NotFound));
        // Fuera de la lista, de otro agente o de otro celular.
        std::fs::write(root.join("otro.txt"), b"x").unwrap();
        assert_eq!(check_fetch(&offered, device, "a", "otro.txt"), Err(Refusal::NotOffered));
        assert_eq!(check_fetch(&offered, device, "b", "nota.txt"), Err(Refusal::NotOffered));
        assert_eq!(check_fetch(&offered, other, "a", "nota.txt"), Err(Refusal::NotOffered));

        // Un symlink que sale del proyecto no pasa (en Windows crearlo pide permisos: si no
        // se puede, se prueba solo lo demás).
        #[cfg(unix)]
        let linked = std::os::unix::fs::symlink(outside.join("ajeno.txt"), root.join("enlace.txt")).is_ok();
        #[cfg(windows)]
        let linked = std::os::windows::fs::symlink_file(outside.join("ajeno.txt"), root.join("enlace.txt")).is_ok();
        if linked {
            let offered = offer(&root, device, &["enlace.txt"]);
            assert_eq!(check_fetch(&offered, device, "a", "enlace.txt"), Err(Refusal::Outside));
        }
        #[cfg(unix)]
        let to_secret = std::os::unix::fs::symlink(root.join(".env"), root.join("inocente.txt")).is_ok();
        #[cfg(windows)]
        let to_secret = std::os::windows::fs::symlink_file(root.join(".env"), root.join("inocente.txt")).is_ok();
        if to_secret {
            let offered = offer(&root, device, &["inocente.txt"]);
            assert_eq!(check_fetch(&offered, device, "a", "inocente.txt"), Err(Refusal::Blocked));
        }
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&outside);
    }

    #[test]
    fn a_un_celular_viejo_no_le_llegan_las_herramientas() {
        use crate::{AgentEntry, EntryKind, ToolCall, ToolStatus};
        let tool = |text: &str, status| AgentEntry {
            kind: EntryKind::Tool,
            text: text.into(),
            images: vec![],
            tool: Some(ToolCall { id: "t1".into(), verb: "Bash".into(), target: "ls".into(), status, detail: Some("x".repeat(20_000)), diff: None, truncated: false }),
        };
        let entries = vec![tool("Corriendo ls", ToolStatus::Running), tool("", ToolStatus::Done)];
        let old = tools_for(entries.clone(), false);
        assert_eq!(old.len(), 1);
        assert!(old[0].tool.is_none());
        let new = tools_for(entries, true);
        assert_eq!(new.len(), 2);
        assert!(new.iter().all(|e| e.tool.as_ref().is_some_and(|t| t.truncated)));
    }
}
