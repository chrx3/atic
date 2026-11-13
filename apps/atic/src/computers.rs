//! El mouse y el teclado de este computador en otro, como Universal Control
//! (`atic-kvm`): el cursor sale por un borde de la pantalla y sigue en el otro
//! equipo, con el teclado detrás. También el portapapeles compartido.
//!
//! El servicio corre en su propio runtime de tokio, como el canal del celular
//! (`phone.rs`), y siempre recibe: el mouse de un equipo vinculado puede pasar
//! a este. Apagado por defecto: arranca al abrir la pill solo si ya hay
//! equipos vinculados (o si se prendió en Ajustes → Computadores). La pill de
//! desarrollo no lo prende sola: usa la misma identidad que la instalada.
//!
//! Usa la carpeta `kvm` de los datos de Atic, la misma del ejecutable de
//! prueba (`atic-kvm`): la identidad y los equipos vinculados se conservan.
//! Lo propio de la pill (con quién se comparte, de qué lado está cada uno y
//! el portapapeles) va en `kvm/share.json`.
//!
//! Ajustes y el panel del notch (`computers_notch.rs`) miran el estado con
//! [`status`]; cuando otro equipo pide vincularse, la pill abre el notch en
//! Computadores para mostrar el código ([`take_pair_code`]).
//!
//! El tab de la pill dice dónde anda el cursor («En Mac de Chris», «Desde
//! LENOVO…») y avisa corto lo que pasa (conectado, sin conexión, lo copiado
//! en el otro) en el lugar del reloj de grabar: [`tab_status`].

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

use atic_core::{AppDirs, MutexExt};
use atic_kvm::files::Purpose;
use atic_kvm::{EndpointId, Event, Os, PairingState, Placement, Service, Share, Side};
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, oneshot};

use crate::i18n::{t, tf};

/// Cuánto se escucha la red al buscar computadores.
const SEARCH: Duration = Duration::from_secs(4);
/// Lo que vale el código para vincular (lo dice Ajustes: «valen por un minuto»).
const CODE_VALID: Duration = atic_kvm::link::CODE_TIMEOUT;
/// Lo que este equipo acepta pedidos de vínculo después de que el usuario
/// toca «Permitir vincular», prende la sección o busca computadores: fuera de
/// eso, el servicio los rechaza sin mostrar nada.
const PAIR_OPEN: Duration = Duration::from_secs(5 * 60);
/// Lo que dura un aviso corto en el tab.
const NOTE_FOR: Duration = Duration::from_secs(4);
/// Un mismo aviso (del mismo equipo y del mismo tipo) no se repite antes de
/// esto: una conexión que se corta y vuelve, o copiar seguido, no llena el tab.
const NOTE_QUIET: Duration = Duration::from_secs(30);
/// Lo más largo de un nombre en el tab (el tab crece con el texto).
const TAB_NAME: usize = 12;
/// El punto del tab: dónde anda el cursor, en azul; lo que llega, en verde;
/// lo que se cortó, en gris.
pub(crate) const TAB_BLUE: u32 = 0x86b6ff;
pub(crate) const TAB_GREEN: u32 = 0x6cc48a;
pub(crate) const TAB_GRAY: u32 = 0x9a9a90;
const TAB_RED: u32 = 0xe8826a;

/// Lo propio de la pill, en `kvm/share.json`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct Prefs {
    /// `None`: nunca se tocó el interruptor (arranca sola si hay equipos vinculados).
    enabled: Option<bool>,
    /// El id del equipo con que se comparte este mouse y teclado.
    share: Option<String>,
    /// De qué lado de este está cada equipo vinculado (por su id) y cuándo
    /// se eligió, aquí o allá. Antes se guardaba solo el lado: se lee igual.
    sides: BTreeMap<String, Placement>,
    clipboard: bool,
}

impl Default for Prefs {
    fn default() -> Self {
        Self { enabled: None, share: None, sides: BTreeMap::new(), clipboard: true }
    }
}

impl Prefs {
    /// De qué lado está `id`, si alguien lo eligió.
    fn side(&self, id: &str) -> Option<Side> {
        self.sides.get(id).map(|p| p.side)
    }
}

/// Un aviso para la sección de Ajustes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Notice {
    Info(String),
    Error(String),
    /// Compartir necesita el permiso de Accesibilidad o de Monitoreo de entrada.
    Permission,
}

/// Un equipo vinculado, como lo muestra Ajustes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeerRow {
    pub id: String,
    pub name: String,
    pub mac: bool,
    pub connected: bool,
    /// De qué lado de este está; `None` si nadie lo eligió.
    pub side: Option<Side>,
    /// Lo que dice el otro (con una versión anterior, que no se pone de
    /// acuerdo sola) cuando no coincide: de qué lado de este dice estar.
    pub claim: Option<Side>,
    /// Este mouse y teclado se usan en él.
    pub sharing: bool,
}

/// Si este equipo atiende a otro que pida vincularse, como lo muestra Ajustes.
/// En minutos, hacia arriba: «queda 1 min» hasta que se cierra.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PairWindow {
    #[default]
    Closed,
    Open { minutes: u64 },
    /// Hubo varios intentos fallidos: no se atiende a nadie por un rato.
    Locked { minutes: u64 },
}

impl From<PairingState> for PairWindow {
    fn from(state: PairingState) -> Self {
        let minutes = |left: Duration| (left.as_millis() as u64).div_ceil(60_000).max(1);
        match state {
            PairingState::Closed => Self::Closed,
            PairingState::Open(left) => Self::Open { minutes: minutes(left) },
            PairingState::Locked(left) => Self::Locked { minutes: minutes(left) },
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Status {
    /// El interruptor de la sección.
    pub enabled: bool,
    pub running: bool,
    pub starting: bool,
    /// El id de este equipo (para vincular sin la búsqueda).
    pub id: String,
    pub peers: Vec<PeerRow>,
    pub clipboard: bool,
    /// Otro equipo quiere vincularse: su nombre y el código que hay que mostrar aquí.
    pub pair_code: Option<(String, String)>,
    /// Si se atiende a un equipo que pida vincularse con este.
    pub pair_window: PairWindow,
    /// Vinculando con otro: su nombre y si ya hay que escribir su código.
    pub pairing: Option<(String, bool)>,
    pub searching: bool,
    /// Lo que encontró la última búsqueda (id y nombre), sin los ya vinculados.
    pub nearby: Option<Vec<(String, String)>>,
    pub notice: Option<Notice>,
    /// El cursor de este equipo anda en otro (su nombre).
    pub away: Option<String>,
    /// El mouse de otro equipo anda en este (su nombre).
    pub controlled: Option<String>,
}

/// En qué anda todo, en una frase: lo primero del vistazo de la tira.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Headline {
    Off,
    Starting,
    /// Prendido y sin equipos vinculados.
    NoPeers,
    /// Hay vinculados, pero ninguno conectado.
    Offline,
    Connected(String),
    /// El cursor de este anda en ese.
    Away(String),
    /// Ese está usando el mouse y teclado de este.
    Controlled(String),
}

impl Status {
    /// Lo que más importa primero: dónde anda el cursor y, si no, con quién
    /// hay conexión (con el que se comparte, si está).
    pub fn headline(&self) -> Headline {
        if !self.enabled {
            return Headline::Off;
        }
        if !self.running {
            return Headline::Starting;
        }
        if self.peers.is_empty() {
            return Headline::NoPeers;
        }
        if let Some(name) = &self.away {
            return Headline::Away(name.clone());
        }
        if let Some(name) = &self.controlled {
            return Headline::Controlled(name.clone());
        }
        let connected = self.peers.iter().filter(|p| p.connected);
        match connected.clone().find(|p| p.sharing).or_else(|| connected.clone().next()) {
            Some(peer) => Headline::Connected(peer.name.clone()),
            None => Headline::Offline,
        }
    }
}

struct Running {
    rt: tokio::runtime::Runtime,
    service: Arc<Service>,
}

struct Pairing {
    id: EndpointId,
    name: String,
    /// El otro ya muestra su código: falta escribirlo.
    asking: bool,
    code: Option<oneshot::Sender<String>>,
    task: Option<tokio::task::AbortHandle>,
}

#[derive(Default)]
struct State {
    /// El código para mostrar y cuándo llegó (vence en `CODE_VALID`).
    pair_code: Option<(String, String, Instant)>,
    pairing: Option<Pairing>,
    searching: bool,
    nearby: Option<Vec<(String, String)>>,
    notice: Option<Notice>,
    live: Live,
}

/// Los avisos cortos del tab.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum NoteKind {
    Connected,
    Disconnected,
    Text,
    Image,
    /// Se están yendo archivos al otro.
    Sending,
    /// Llegaron archivos del otro a Descargas (arrastrados o mandados).
    Downloaded,
    /// Llegaron archivos copiados en el otro: listos para pegar.
    FilesReady,
    FilesFailed,
}

/// Un aviso corto: qué, de quién, cuántos archivos y dónde quedaron (para
/// mostrarlos en su carpeta con un clic en el tab).
#[derive(Clone, Debug)]
struct Said {
    kind: NoteKind,
    name: String,
    count: usize,
    path: Option<PathBuf>,
}

impl Said {
    fn new(kind: NoteKind, name: &str) -> Self {
        Self { kind, name: name.to_string(), count: 0, path: None }
    }
}

/// Lo que dice el tab: dónde anda el cursor y el último aviso corto.
#[derive(Default)]
struct Live {
    /// El cursor de este equipo está en el otro (id y nombre).
    away: Option<(EndpointId, String)>,
    /// El mouse del otro está en este.
    controlled: Option<(EndpointId, String)>,
    /// El aviso a la vista, de quién y desde cuándo.
    note: Option<(Said, Instant)>,
    /// Cuándo se avisó por última vez cada cosa de cada equipo.
    said: HashMap<(EndpointId, NoteKind), Instant>,
    /// Lo copiado en el otro mientras el cursor estaba allá: se avisa al volver
    /// (solo lo último).
    copied_away: Option<(EndpointId, Said)>,
}

impl Live {
    /// Avisa, salvo que lo mismo del mismo equipo se haya dicho hace poco.
    fn note(&mut self, id: EndpointId, name: &str, kind: NoteKind, now: Instant) {
        self.say(id, Said::new(kind, name), now);
    }

    fn say(&mut self, id: EndpointId, said: Said, now: Instant) {
        if self.said.get(&(id, said.kind)).is_some_and(|at| now.duration_since(*at) < NOTE_QUIET) {
            return;
        }
        self.said.insert((id, said.kind), now);
        self.note = Some((said, now));
    }

    /// Lo que llega del otro: con el cursor allá no se ve el tab, así que se
    /// dice al volver (solo lo último).
    fn arrived(&mut self, id: EndpointId, said: Said, now: Instant) {
        if self.away.is_some() {
            self.copied_away = Some((id, said));
        } else {
            self.say(id, said, now);
        }
    }

    /// El aviso a la vista, si sigue.
    fn showing(&self, now: Instant) -> Option<&Said> {
        self.note.as_ref().filter(|(_, at)| now.duration_since(*at) < NOTE_FOR).map(|(said, _)| said)
    }

    fn event(&mut self, event: &Event, now: Instant) {
        match event {
            Event::Connected { id, name } => self.note(*id, name, NoteKind::Connected, now),
            Event::Disconnected { id, name } => {
                // Se cortó: el cursor ya no anda allá ni el suyo aquí.
                if self.away.as_ref().is_some_and(|(away, _)| away == id) {
                    self.away = None;
                }
                if self.controlled.as_ref().is_some_and(|(by, _)| by == id) {
                    self.controlled = None;
                }
                self.note(*id, name, NoteKind::Disconnected, now);
            }
            Event::Away { id, name } => self.away = Some((*id, name.clone())),
            Event::Home => {
                self.away = None;
                if let Some((id, said)) = self.copied_away.take() {
                    self.say(id, said, now);
                }
            }
            Event::Controlled { id, name, on } => {
                if *on {
                    self.controlled = Some((*id, name.clone()));
                } else if self.controlled.as_ref().is_some_and(|(by, _)| by == id) {
                    self.controlled = None;
                }
            }
            Event::Clipboard { id, name, image } => {
                let kind = if *image { NoteKind::Image } else { NoteKind::Text };
                self.arrived(*id, Said::new(kind, name), now);
            }
            Event::FilesSending { id, name, count, .. } => {
                self.say(*id, Said { count: *count, ..Said::new(NoteKind::Sending, name) }, now);
            }
            Event::FilesReceived { id, name, paths, purpose } => {
                let kind = match purpose {
                    Purpose::Drop => NoteKind::Downloaded,
                    Purpose::Clipboard => NoteKind::FilesReady,
                };
                let said = Said { count: paths.len(), path: paths.first().cloned(), ..Said::new(kind, name) };
                self.arrived(*id, said, now);
            }
            // Sin de quién: se dice siempre (pasa poco y hay que saberlo).
            Event::FilesFailed { .. } => self.note = Some((Said::new(NoteKind::FilesFailed, ""), now)),
            _ => {}
        }
    }

    /// El texto del tab y el color de su punto. Un aviso a la vista va primero;
    /// después, dónde anda el cursor.
    fn tab(&self, now: Instant) -> Option<(String, u32)> {
        if let Some(said) = self.showing(now) {
            let name = short_name(&said.name);
            let one = said.count <= 1;
            let (key, color) = match said.kind {
                NoteKind::Connected => ("pill.kvm.tabConnected", TAB_GREEN),
                NoteKind::Disconnected => ("pill.kvm.tabDisconnected", TAB_GRAY),
                NoteKind::Text => ("pill.kvm.tabText", TAB_GREEN),
                NoteKind::Image => ("pill.kvm.tabImage", TAB_GREEN),
                NoteKind::Sending if one => ("pill.kvm.tabSendingOne", TAB_BLUE),
                NoteKind::Sending => ("pill.kvm.tabSending", TAB_BLUE),
                NoteKind::Downloaded if one => ("pill.kvm.tabDownloadedOne", TAB_GREEN),
                NoteKind::Downloaded => ("pill.kvm.tabDownloaded", TAB_GREEN),
                NoteKind::FilesReady if one => ("pill.kvm.tabFilesReadyOne", TAB_GREEN),
                NoteKind::FilesReady => ("pill.kvm.tabFilesReady", TAB_GREEN),
                NoteKind::FilesFailed => ("pill.kvm.tabFilesFailed", TAB_RED),
            };
            return Some((tf(key, &[("name", &name), ("count", &said.count)]), color));
        }
        if let Some((_, name)) = &self.away {
            return Some((tf("pill.kvm.tabAway", &[("name", &short_name(name))]), TAB_BLUE));
        }
        self.controlled.as_ref().map(|(_, name)| (tf("pill.kvm.tabControlled", &[("name", &short_name(name))]), TAB_BLUE))
    }
}

/// El nombre cortado para el tab: «LENOVOIDEAPA…».
fn short_name(name: &str) -> String {
    let name = name.trim();
    if name.chars().count() <= TAB_NAME {
        return name.to_string();
    }
    let cut: String = name.chars().take(TAB_NAME - 1).collect();
    format!("{}…", cut.trim_end())
}

static RUNNING: Mutex<Option<Running>> = Mutex::new(None);
/// Arrancando (abrir la conexión tarda un poco): no se arranca dos veces.
static STARTING: AtomicBool = AtomicBool::new(false);
static STATE: LazyLock<Mutex<State>> = LazyLock::new(|| Mutex::new(State::default()));
/// Llegó un código para mostrar: la pill abre el notch en Computadores.
static PAIR_CODE_NEW: AtomicBool = AtomicBool::new(false);
/// El usuario acaba de prender la sección: al arrancar, se abre el vínculo.
static PAIR_ON_START: AtomicBool = AtomicBool::new(false);
/// `share.json` se lee y se escribe de a uno.
static PREFS: Mutex<()> = Mutex::new(());

fn dir() -> Option<PathBuf> {
    Some(AppDirs::new().ok()?.data_dir().join("kvm"))
}

fn load_prefs() -> Prefs {
    dir()
        .and_then(|dir| std::fs::read(dir.join("share.json")).ok())
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

/// Lee, cambia y guarda `share.json`; devuelve cómo quedó.
fn edit_prefs(apply: impl FnOnce(&mut Prefs)) -> Prefs {
    let _guard = PREFS.lock_or_recover();
    let mut prefs = load_prefs();
    apply(&mut prefs);
    if let Some(dir) = dir() {
        let _ = std::fs::create_dir_all(&dir);
        match serde_json::to_string_pretty(&prefs) {
            Ok(json) => {
                if let Err(error) = atic_core::write_atomic_str(&dir.join("share.json"), &json) {
                    tracing::warn!(%error, "computadores: no se pudo guardar share.json");
                }
            }
            Err(error) => tracing::warn!(%error, "computadores: share.json"),
        }
    }
    prefs
}

/// Si ya hay equipos vinculados (`peers.json` del servicio), sin abrir la conexión.
fn has_peers() -> bool {
    #[derive(Deserialize)]
    struct PeerFile {
        #[serde(default)]
        peers: BTreeMap<String, serde_json::Value>,
    }
    dir()
        .and_then(|dir| std::fs::read(dir.join("peers.json")).ok())
        .and_then(|bytes| serde_json::from_slice::<PeerFile>(&bytes).ok())
        .is_some_and(|file| !file.peers.is_empty())
}

/// Al abrir la pill: prende el servicio si ya hay equipos vinculados y no se apagó.
pub fn start() {
    let prefs = load_prefs();
    let wanted = match prefs.enabled {
        Some(on) => on,
        // La de desarrollo no se prende sola: la instalada usa la misma identidad.
        None => !crate::dev::on() && has_peers(),
    };
    if wanted {
        launch();
    }
}

/// Al salir: el cursor vuelve a este equipo y se cortan las sesiones avisando.
pub fn stop() {
    if let Some(running) = RUNNING.lock_or_recover().take() {
        shutdown(running);
    }
}

fn shutdown(running: Running) {
    let Running { rt, service } = running;
    {
        let _enter = rt.enter();
        service.stop();
    }
    rt.block_on(async {
        let _ = tokio::time::timeout(Duration::from_millis(300), service.node().endpoint.close()).await;
    });
    // Las tareas del servicio (y el hilo que mira el portapapeles) terminan con el runtime.
    rt.shutdown_background();
}

/// Abre la conexión en un hilo aparte (tarda un poco).
fn launch() {
    if RUNNING.lock_or_recover().is_some() || STARTING.swap(true, Ordering::SeqCst) {
        return;
    }
    let spawned = std::thread::Builder::new().name("computadores".into()).spawn(|| {
        if let Err(error) = open() {
            tracing::warn!(%error, "computadores: no arrancó");
            set_notice(Some(Notice::Error(error)));
        }
        STARTING.store(false, Ordering::SeqCst);
    });
    if spawned.is_err() {
        STARTING.store(false, Ordering::SeqCst);
    }
}

fn open() -> Result<(), String> {
    let dir = dir().ok_or("sin carpeta de datos")?;
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .thread_name("computadores")
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    let name = crate::phone::computer_name().unwrap_or_else(|| t("pill.kvm.thisComputer").to_string());
    let (events, rx) = mpsc::unbounded_channel();
    let service = Arc::new(rt.block_on(Service::start_in_keychain(&dir, name, events)).map_err(|e| format!("{e:#}"))?);
    let prefs = load_prefs();
    // Se apagó mientras arrancaba.
    if prefs.enabled == Some(false) {
        shutdown(Running { rt, service });
        return Ok(());
    }
    service.set_clipboard(prefs.clipboard);
    // Antes se compartía por la derecha si nunca se eligió el lado: queda elegido así.
    let prefs = match prefs.share.clone().filter(|id| !prefs.sides.contains_key(id)) {
        Some(id) => edit_prefs(|prefs| {
            prefs.sides.insert(id, Placement { side: Side::Right, rev: 0 });
        }),
        None => prefs,
    };
    service.set_placements(prefs.sides.iter().filter_map(|(id, placement)| Some((id.parse::<EndpointId>().ok()?, *placement))));
    if PAIR_ON_START.swap(false, Ordering::SeqCst) {
        service.open_pairing(PAIR_OPEN);
    }
    rt.spawn(handle_events(service.clone(), rx));
    // Seguir compartiendo con quien se compartía (si sigue vinculado).
    let saved = prefs.share.as_deref().and_then(|id| id.parse::<EndpointId>().ok());
    if let Some(peer) = saved.filter(|peer| service.peers().iter().any(|(id, _)| id == peer)) {
        if let Some(placement) = service.placement(&peer) {
            let _enter = rt.enter();
            if service.share(Some(Share { peer, side: placement.side })).is_err() {
                set_notice(Some(Notice::Permission));
            }
        }
    }
    *RUNNING.lock_or_recover() = Some(Running { rt, service });
    Ok(())
}

/// Hace algo con el servicio dentro de su runtime (`share` lanza tareas).
fn with_service<T>(apply: impl FnOnce(&Arc<Service>) -> T) -> Option<T> {
    let guard = RUNNING.lock_or_recover();
    let running = guard.as_ref()?;
    let _enter = running.rt.enter();
    Some(apply(&running.service))
}

fn set_notice(notice: Option<Notice>) {
    STATE.lock_or_recover().notice = notice;
}

async fn handle_events(service: Arc<Service>, mut rx: mpsc::UnboundedReceiver<Event>) {
    while let Some(event) = rx.recv().await {
        STATE.lock_or_recover().live.event(&event, Instant::now());
        match event {
            Event::PairCode { name, code } => {
                STATE.lock_or_recover().pair_code = Some((name, code, Instant::now()));
                PAIR_CODE_NEW.store(true, Ordering::SeqCst);
            }
            Event::Paired { id, name } => {
                let mut state = STATE.lock_or_recover();
                state.pair_code = None;
                state.pairing = None;
                if let Some(found) = state.nearby.as_mut() {
                    found.retain(|(other, _)| *other != id.to_string());
                }
                state.notice = Some(Notice::Info(tf("pill.kvm.paired", &[("name", &name)])));
            }
            Event::PairFailed { reason } => {
                let mut state = STATE.lock_or_recover();
                state.pair_code = None;
                state.pairing = None;
                state.notice = Some(Notice::Error(tf("pill.kvm.pairFailed", &[("reason", &reason)])));
            }
            Event::Connected { name, .. } => tracing::info!("computadores: «{name}» conectado"),
            Event::Disconnected { name, .. } => tracing::info!("computadores: «{name}» se desconectó"),
            // Dónde anda el cursor y lo copiado allá: los lleva `Live` (el tab).
            Event::Away { .. } | Event::Home | Event::Controlled { .. } | Event::Clipboard { .. } => {}
            // Los archivos que van y vienen: también los dice el tab (`Live`).
            Event::FilesSending { .. } | Event::FilesSent { .. } => {}
            Event::FilesReceived { name, paths, .. } => tracing::info!("computadores: {} archivo(s) de «{name}»", paths.len()),
            Event::FilesFailed { reason } => {
                tracing::warn!(%reason, "computadores: archivos");
                set_notice(Some(Notice::Error(tf("pill.kvm.filesFailed", &[("reason", &reason)]))));
            }
            // El lado se eligió allá después que aquí (el servicio ya lo sigue): se guarda.
            Event::PeerLayout { id, side, rev } => {
                edit_prefs(|prefs| {
                    prefs.sides.insert(id.to_string(), Placement { side, rev });
                });
            }
            // El otro comparte su mouse y teclado con este: si este no comparte
            // con nadie, comparte de vuelta por el lado acordado.
            Event::PeerShares { id, side } => {
                if service.sharing().is_none() {
                    match service.share(Some(Share { peer: id, side })) {
                        Ok(()) => {
                            edit_prefs(|prefs| prefs.share = Some(id.to_string()));
                        }
                        Err(_) => set_notice(Some(Notice::Permission)),
                    }
                }
            }
            // Lo único que avisa así es que falta el permiso para escuchar el teclado.
            Event::Error(error) => {
                tracing::warn!(%error, "computadores");
                set_notice(Some(Notice::Permission));
            }
        }
    }
}

/// Si llegó un código para mostrar desde la última vez que se preguntó.
pub fn take_pair_code() -> bool {
    PAIR_CODE_NEW.swap(false, Ordering::SeqCst)
}

/// Los archivos del aviso a la vista en el tab (los que llegaron del otro),
/// para mostrarlos en su carpeta con un clic.
pub fn tab_reveal() -> Option<PathBuf> {
    STATE.lock_or_recover().live.showing(Instant::now()).and_then(|said| said.path.clone())
}

/// Manda archivos (soltados en el panel o el vistazo) a las Descargas de `id`;
/// abre la sesión con él si hace falta. El tab avisa cuando empiezan a irse.
pub fn send_files(id: &str, paths: Vec<PathBuf>) -> bool {
    let Ok(peer) = id.parse::<EndpointId>() else { return false };
    if paths.is_empty() {
        return false;
    }
    let guard = RUNNING.lock_or_recover();
    let Some(running) = guard.as_ref() else { return false };
    let service = running.service.clone();
    running.rt.spawn(async move {
        match service.connect(peer).await {
            Ok(()) => service.send_files(peer, paths),
            Err(error) => {
                tracing::warn!(error = %format!("{error:#}"), "computadores: no se pudo conectar para mandar archivos");
                set_notice(Some(Notice::Error(t("pill.kvm.sendOffline").to_string())));
            }
        }
    });
    true
}

/// Lo que dice el tab de la pill ahora (texto y color del punto), si algo.
/// Barato: la pill lo pregunta seguido.
pub fn tab_status() -> Option<(String, u32)> {
    STATE.lock_or_recover().live.tab(Instant::now())
}

pub fn status() -> Status {
    let prefs = load_prefs();
    let starting = STARTING.load(Ordering::SeqCst);
    let (running, id, peers, pair_window) = {
        let guard = RUNNING.lock_or_recover();
        match guard.as_ref() {
            Some(running) => {
                let service = &running.service;
                let connected = service.connected();
                let sharing = service.sharing().map(|s| s.peer);
                let mut peers: Vec<PeerRow> = service
                    .peers()
                    .into_iter()
                    .map(|(id, peer)| PeerRow {
                        id: id.to_string(),
                        side: service.placement(&id).map(|p| p.side),
                        claim: service.claim(&id),
                        name: peer.name,
                        mac: peer.os == Os::Macos,
                        connected: connected.contains(&id),
                        sharing: sharing == Some(id),
                    })
                    .collect();
                peers.sort_by_key(|p| p.name.to_lowercase());
                (true, service.id().to_string(), peers, PairWindow::from(service.pairing_state()))
            }
            None => (false, String::new(), Vec::new(), PairWindow::Closed),
        }
    };
    let state = STATE.lock_or_recover();
    Status {
        enabled: running || starting || prefs.enabled == Some(true),
        running,
        starting,
        id,
        peers,
        clipboard: prefs.clipboard,
        pair_code: state
            .pair_code
            .as_ref()
            .filter(|(_, _, at)| at.elapsed() < CODE_VALID)
            .map(|(name, code, _)| (name.clone(), code.clone())),
        pair_window,
        pairing: state.pairing.as_ref().map(|p| (p.name.clone(), p.asking)),
        searching: state.searching,
        nearby: state.nearby.clone(),
        notice: state.notice.clone(),
        away: state.live.away.as_ref().map(|(_, name)| name.clone()),
        controlled: state.live.controlled.as_ref().map(|(_, name)| name.clone()),
    }
}

/// Prende o apaga todo (el interruptor de la sección).
pub fn set_enabled(on: bool) {
    edit_prefs(|prefs| prefs.enabled = Some(on));
    if on {
        set_notice(None);
        // Quien lo prende a mano está por vincular: se aceptan pedidos un rato.
        if with_service(|service| service.open_pairing(PAIR_OPEN)).is_none() {
            PAIR_ON_START.store(true, Ordering::SeqCst);
        }
        launch();
        return;
    }
    PAIR_ON_START.store(false, Ordering::SeqCst);
    let running = RUNNING.lock_or_recover().take();
    {
        let mut state = STATE.lock_or_recover();
        if let Some(task) = state.pairing.take().and_then(|p| p.task) {
            task.abort();
        }
        *state = State::default();
    }
    // Cortar las sesiones espera un poco: fuera del hilo de la ventana.
    if let Some(running) = running {
        let _ = std::thread::Builder::new().name("computadores-stop".into()).spawn(move || shutdown(running));
    }
}

/// «Permitir vincular»: por un rato se atiende a un equipo que pida vincularse
/// con este. Se cierra solo al vincularse uno o al pasar el plazo.
pub fn open_pairing() {
    with_service(|service| service.open_pairing(PAIR_OPEN));
}

pub fn close_pairing() {
    with_service(|service| service.close_pairing());
}

/// Busca los equipos con Atic abierto en la red. Bloquea unos segundos:
/// llamar fuera del hilo de la ventana.
pub fn search() {
    let found = {
        let guard = RUNNING.lock_or_recover();
        let Some(running) = guard.as_ref() else { return };
        (running.rt.handle().clone(), running.service.clone())
    };
    let (handle, service) = found;
    // Buscar es estar vinculando: el otro equipo también puede pedirlo a este.
    service.open_pairing(PAIR_OPEN);
    {
        let mut state = STATE.lock_or_recover();
        state.searching = true;
        state.notice = None;
    }
    let found = handle.block_on(service.nearby(SEARCH));
    let paired: Vec<EndpointId> = service.peers().into_iter().map(|(id, _)| id).collect();
    let found = found.into_iter().filter(|(id, _)| !paired.contains(id)).map(|(id, name)| (id.to_string(), name)).collect();
    let mut state = STATE.lock_or_recover();
    state.searching = false;
    state.nearby = Some(found);
}

/// Pide vincularse con `id` (de la búsqueda o pegado a mano). Cuando el otro
/// muestra su código, Ajustes pide escribirlo ([`submit_code`]).
pub fn pair(id: &str, name: Option<String>) -> Result<(), String> {
    let id: EndpointId = id.trim().parse().map_err(|_| t("pill.kvm.badId").to_string())?;
    let guard = RUNNING.lock_or_recover();
    let running = guard.as_ref().ok_or_else(|| t("pill.kvm.off").to_string())?;
    if running.service.id() == id {
        return Err(t("pill.kvm.ownIdError").to_string());
    }
    let name = name.unwrap_or_else(|| t("pill.kvm.theOther").to_string());
    let (code_tx, code_rx) = oneshot::channel::<String>();
    {
        let mut state = STATE.lock_or_recover();
        if let Some(task) = state.pairing.take().and_then(|p| p.task) {
            task.abort();
        }
        state.notice = None;
        state.pairing = Some(Pairing { id, name, asking: false, code: Some(code_tx), task: None });
    }
    let service = running.service.clone();
    let task = running.rt.spawn(async move {
        // Se espera recién cuando el otro ya muestra su código.
        let code = async move {
            if let Some(pairing) = STATE.lock_or_recover().pairing.as_mut().filter(|p| p.id == id) {
                pairing.asking = true;
            }
            code_rx.await.map_err(|_| anyhow::anyhow!("cancelado"))
        };
        let _ = service.pair(id, code).await;
    });
    if let Some(pairing) = STATE.lock_or_recover().pairing.as_mut().filter(|p| p.id == id) {
        pairing.task = Some(task.abort_handle());
    }
    Ok(())
}

/// El código que muestra el otro equipo.
pub fn submit_code(code: &str) {
    let code: String = code.chars().filter(char::is_ascii_digit).collect();
    let mut state = STATE.lock_or_recover();
    if let Some(pairing) = state.pairing.as_mut() {
        if let Some(tx) = pairing.code.take() {
            let _ = tx.send(code);
            pairing.asking = false;
        }
    }
}

pub fn cancel_pair() {
    let mut state = STATE.lock_or_recover();
    if let Some(task) = state.pairing.take().and_then(|p| p.task) {
        task.abort();
    }
}

/// Usar (o dejar de usar) este mouse y teclado en `id`. Para usarlo hay que
/// saber de qué lado está.
pub fn set_sharing(id: &str, on: bool) {
    let Ok(peer) = id.parse::<EndpointId>() else { return };
    let side = with_service(|service| service.placement(&peer).map(|p| p.side)).flatten().or_else(|| load_prefs().side(id));
    let share = match (on, side) {
        (true, Some(side)) => Some(Share { peer, side }),
        (true, None) => {
            set_notice(Some(Notice::Info(t("pill.kvm.pickSideFirst").to_string())));
            return;
        }
        (false, _) => None,
    };
    match with_service(|service| service.share(share)) {
        Some(Ok(())) => {
            set_notice(None);
            edit_prefs(|prefs| prefs.share = on.then(|| id.to_string()));
        }
        Some(Err(_)) => set_notice(Some(Notice::Permission)),
        None => {}
    }
}

/// De qué lado de este está `id`. Si se comparte con él, sigue con el lado
/// nuevo, y se le avisa al otro para que allá quede igual. No empieza a
/// compartir (eso es el interruptor).
pub fn set_side(id: &str, side: Side) {
    let Ok(peer) = id.parse::<EndpointId>() else { return };
    let placement = with_service(|service| service.set_side(peer, side)).unwrap_or_else(|| {
        let rev = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64);
        Placement { side, rev }
    });
    edit_prefs(|prefs| {
        prefs.sides.insert(id.to_string(), placement);
    });
    if matches!(STATE.lock_or_recover().notice, Some(Notice::Info(_))) {
        set_notice(None);
    }
}

/// «Usar ese»: este equipo toma el lado que dice el otro.
pub fn use_claim(id: &str) {
    let Ok(peer) = id.parse::<EndpointId>() else { return };
    if let Some(side) = with_service(|service| service.claim(&peer)).flatten() {
        set_side(id, side);
    }
}

/// Deja de aceptar a `id` (hay que volver a vincularse).
pub fn forget(id: &str) {
    let Ok(peer) = id.parse::<EndpointId>() else { return };
    if let Some(Err(error)) = with_service(|service| service.forget(&peer)) {
        set_notice(Some(Notice::Error(format!("{error:#}"))));
        return;
    }
    edit_prefs(|prefs| {
        prefs.sides.remove(id);
        if prefs.share.as_deref() == Some(id) {
            prefs.share = None;
        }
    });
}

pub fn set_clipboard(on: bool) {
    edit_prefs(|prefs| prefs.clipboard = on);
    with_service(|service| service.set_clipboard(on));
}

pub fn dismiss_notice() {
    set_notice(None);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn share_json_sin_campos_trae_el_portapapeles_prendido() {
        let prefs: Prefs = serde_json::from_str("{}").unwrap();
        assert_eq!(prefs, Prefs::default());
        assert!(prefs.clipboard);
        assert_eq!(prefs.enabled, None);
    }

    #[test]
    fn el_lado_se_guarda_por_equipo() {
        let mut prefs = Prefs::default();
        // Nadie lo eligió: no se inventa uno.
        assert_eq!(prefs.side("a"), None);
        prefs.sides.insert("a".into(), Placement { side: Side::Top, rev: 7 });
        let json = serde_json::to_string(&prefs).unwrap();
        assert!(json.contains(r#""a":{"side":"top","rev":7}"#), "{json}");
        let back: Prefs = serde_json::from_str(&json).unwrap();
        assert_eq!(back.side("a"), Some(Side::Top));
        assert_eq!(back.sides["a"].rev, 7);
        assert_eq!(back.side("b"), None);
    }

    /// El `share.json` de antes guardaba solo el lado: se lee con `rev` 0 y
    /// no se pierde nada más.
    #[test]
    fn share_json_viejo_se_lee() {
        let old = r#"{"enabled":true,"share":"a","sides":{"a":"left","b":"bottom"},"clipboard":false}"#;
        let prefs: Prefs = serde_json::from_str(old).unwrap();
        assert_eq!(prefs.enabled, Some(true));
        assert_eq!(prefs.share.as_deref(), Some("a"));
        assert!(!prefs.clipboard);
        assert_eq!(prefs.sides["a"], Placement { side: Side::Left, rev: 0 });
        assert_eq!(prefs.sides["b"], Placement { side: Side::Bottom, rev: 0 });
        // Mezclado (uno ya guardado con la versión nueva) también.
        let mixed: Prefs = serde_json::from_str(r#"{"sides":{"a":"left","b":{"side":"top","rev":9}}}"#).unwrap();
        assert_eq!(mixed.sides["b"], Placement { side: Side::Top, rev: 9 });
    }

    #[test]
    fn lo_que_queda_para_vincular_se_dice_en_minutos_hacia_arriba() {
        let open = |millis| PairWindow::from(PairingState::Open(Duration::from_millis(millis)));
        assert_eq!(open(299_990), PairWindow::Open { minutes: 5 });
        assert_eq!(open(240_000), PairWindow::Open { minutes: 4 });
        assert_eq!(open(500), PairWindow::Open { minutes: 1 });
        assert_eq!(PairWindow::from(PairingState::Locked(Duration::from_secs(61))), PairWindow::Locked { minutes: 2 });
        assert_eq!(PairWindow::from(PairingState::Closed), PairWindow::Closed);
    }

    /// Dos ids válidos: el neutro y el punto base de ed25519.
    fn ids() -> (EndpointId, EndpointId) {
        let mut one = [0u8; 32];
        one[0] = 1;
        let mut base = [0x66u8; 32];
        base[0] = 0x58;
        (EndpointId::from_bytes(&one).unwrap(), EndpointId::from_bytes(&base).unwrap())
    }

    fn text(live: &Live, now: Instant) -> Option<String> {
        live.tab(now).map(|(text, _)| text)
    }

    fn peer(name: &str, connected: bool, sharing: bool) -> PeerRow {
        PeerRow { id: name.into(), name: name.into(), mac: true, connected, side: Some(Side::Right), claim: None, sharing }
    }

    #[test]
    fn los_archivos_avisan_en_el_tab_y_lo_recibido_se_puede_mostrar() {
        let (mac, _) = ids();
        let now = Instant::now();
        let mut live = Live::default();
        live.event(&Event::FilesSending { id: mac, name: "Mac".into(), count: 3, bytes: 10, purpose: Purpose::Drop }, now);
        let sending = text(&live, now).unwrap();
        assert!(sending.contains("Mac") && sending.contains('3'));
        assert!(live.showing(now).unwrap().path.is_none());
        // Lo que llega con el cursor allá se dice al volver, con su ruta.
        live.event(&Event::Away { id: mac, name: "Mac".into() }, now);
        let file = PathBuf::from("/tmp/informe.pdf");
        live.event(&Event::FilesReceived { id: mac, name: "Mac".into(), paths: vec![file.clone()], purpose: Purpose::Drop }, now);
        assert!(text(&live, now).unwrap().contains("Mac"));
        assert!(live.showing(now).is_some_and(|said| said.kind == NoteKind::Sending));
        let back = now + Duration::from_secs(1);
        live.event(&Event::Home, back);
        assert_eq!(live.showing(back).and_then(|said| said.path.clone()), Some(file));
        assert!(live.showing(back + NOTE_FOR).is_none());
        // Un error se dice aunque no se sepa de quién.
        live.event(&Event::FilesFailed { reason: "x".into() }, back);
        assert!(live.showing(back).is_some_and(|said| said.kind == NoteKind::FilesFailed));
    }

    #[test]
    fn el_vistazo_dice_primero_donde_anda_el_cursor() {
        let mut status = Status::default();
        assert_eq!(status.headline(), Headline::Off);
        status.enabled = true;
        assert_eq!(status.headline(), Headline::Starting);
        status.running = true;
        assert_eq!(status.headline(), Headline::NoPeers);
        status.peers = vec![peer("A", false, false), peer("B", false, false)];
        assert_eq!(status.headline(), Headline::Offline);
        // Conectados los dos: manda el que usa este mouse y teclado.
        status.peers = vec![peer("A", true, false), peer("B", true, true)];
        assert_eq!(status.headline(), Headline::Connected("B".into()));
        status.peers[1].sharing = false;
        assert_eq!(status.headline(), Headline::Connected("A".into()));
        status.controlled = Some("A".into());
        assert_eq!(status.headline(), Headline::Controlled("A".into()));
        status.away = Some("B".into());
        assert_eq!(status.headline(), Headline::Away("B".into()));
        // Apagado gana a todo.
        status.enabled = false;
        assert_eq!(status.headline(), Headline::Off);
    }

    #[test]
    fn el_tab_dice_donde_anda_el_cursor() {
        let (mac, pc) = ids();
        let now = Instant::now();
        let mut live = Live::default();
        assert_eq!(live.tab(now), None);
        live.event(&Event::Away { id: mac, name: "Mac de Chris".into() }, now);
        let away = text(&live, now).unwrap();
        assert!(away.contains("Mac de Chris"));
        live.event(&Event::Home, now);
        assert_eq!(live.tab(now), None);
        live.event(&Event::Controlled { id: pc, name: "LENOVOIDEAPAD3".into(), on: true }, now);
        // El nombre largo se corta: el tab crece con el texto.
        assert!(text(&live, now).unwrap().contains("LENOVOIDEAP…"));
        // Otro equipo que suelta no borra al que sigue aquí.
        live.event(&Event::Controlled { id: mac, name: "Mac de Chris".into(), on: false }, now);
        assert!(live.tab(now).is_some());
        live.event(&Event::Controlled { id: pc, name: "LENOVOIDEAPAD3".into(), on: false }, now);
        assert_eq!(live.tab(now), None);
    }

    #[test]
    fn un_aviso_dura_poco_y_no_se_repite_seguido() {
        let (mac, _) = ids();
        let now = Instant::now();
        let mut live = Live::default();
        live.event(&Event::Connected { id: mac, name: "Mac".into() }, now);
        assert!(live.tab(now).is_some());
        assert_eq!(live.tab(now + NOTE_FOR), None);
        // Se corta y vuelve: avisa que se cortó, pero el «conectado» de nuevo no.
        let later = now + Duration::from_secs(10);
        live.event(&Event::Disconnected { id: mac, name: "Mac".into() }, later);
        let gone = text(&live, later).unwrap();
        live.event(&Event::Connected { id: mac, name: "Mac".into() }, later + Duration::from_secs(1));
        assert_eq!(text(&live, later + Duration::from_secs(1)).unwrap(), gone);
        // Pasado el rato, sí.
        let much_later = now + NOTE_QUIET + Duration::from_secs(1);
        live.event(&Event::Connected { id: mac, name: "Mac".into() }, much_later);
        assert_ne!(text(&live, much_later).unwrap(), gone);
    }

    #[test]
    fn lo_copiado_alla_se_avisa_al_volver_y_una_vez() {
        let (mac, _) = ids();
        let now = Instant::now();
        let mut live = Live::default();
        live.event(&Event::Away { id: mac, name: "Mac".into() }, now);
        let away = text(&live, now).unwrap();
        for i in 0..5 {
            live.event(&Event::Clipboard { id: mac, name: "Mac".into(), image: i == 4 }, now);
        }
        // Mientras el cursor está allá, el tab sigue diciendo dónde.
        assert_eq!(text(&live, now).unwrap(), away);
        let back = now + Duration::from_secs(2);
        live.event(&Event::Home, back);
        let copied = text(&live, back).unwrap();
        assert_ne!(copied, away);
        assert_eq!(live.tab(back + NOTE_FOR), None);
        // Copiar de nuevo enseguida no vuelve a avisar.
        live.event(&Event::Clipboard { id: mac, name: "Mac".into(), image: true }, back + Duration::from_secs(5));
        assert_eq!(live.tab(back + Duration::from_secs(5)), None);
    }
}
