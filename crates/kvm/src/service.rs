//! Todo junto, como lo usa una app (la pill o el ejecutable de prueba): un
//! [`Service`] que siempre recibe (el mouse y el teclado de los equipos
//! vinculados pueden pasar a este) y, si se le dice, comparte el de aquí
//! con uno de ellos ([`Service::share`]). Los dos sentidos a la vez.
//!
//! Por cada equipo vinculado hay a lo más una sesión ([`Link`]): la abre
//! quien comparte, y si los dos se conectan a la vez se queda la que abrió el
//! equipo de id mayor (los dos eligen la misma y cierran la otra). Por ella
//! van el mouse y el teclado de aquí, las respuestas (devolver el cursor) y el
//! portapapeles.
//!
//! De qué lado está cada equipo ([`Placement`]) lo acuerdan los dos: va en el
//! saludo y en [`Msg::Layout`] cuando cambia, y gana el cambio más nuevo (con
//! empate, el del id mayor).
//!
//! Lo que pasa se avisa por [`Event`]s.

use std::collections::HashMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::Result;
use iroh::endpoint::Connection;
use iroh::EndpointId;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::{self, UnboundedSender};
use tokio::task::JoinHandle;

use crate::clip::{Clip, Copied};
use crate::files::{self, Purpose};
use crate::layout::Side;
use crate::link::{Node, PairingState, Peer, Session, PAIR_ALPN};
use crate::proto::{caps, clean_name, read_msg, write_frame, Incoming, Msg, Os, CAP_LAYOUT2, CAP_PING};
use crate::source::Grab;
use crate::sys;
use crate::target::Target;

/// Con qué equipo se comparte este mouse y teclado, y de qué lado de este está.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Share {
    pub peer: EndpointId,
    pub side: Side,
}

/// De qué lado de este equipo está otro y cuándo se eligió (`rev`,
/// milisegundos desde 1970; 0 si no se sabe, como lo guardado antes de que
/// se acordara). Se guarda como `{"side": "right", "rev": 123}`; también se
/// lee el formato de antes, solo el lado (`"right"`), con `rev` 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "SavedPlacement")]
pub struct Placement {
    pub side: Side,
    pub rev: u64,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum SavedPlacement {
    Old(Side),
    New {
        side: Side,
        #[serde(default)]
        rev: u64,
    },
}

impl From<SavedPlacement> for Placement {
    fn from(saved: SavedPlacement) -> Self {
        match saved {
            SavedPlacement::Old(side) => Placement { side, rev: 0 },
            SavedPlacement::New { side, rev } => Placement { side, rev },
        }
    }
}

#[derive(Clone, Debug)]
pub enum Event {
    /// Otro equipo quiere vincularse: mostrar este código en esta pantalla.
    PairCode { name: String, code: String },
    /// Quedó vinculado (desde cualquiera de los dos lados).
    Paired { id: EndpointId, name: String },
    PairFailed { reason: String },
    Connected { id: EndpointId, name: String },
    Disconnected { id: EndpointId, name: String },
    /// De qué lado de este está `id` cambió por lo que dijo el otro (lo
    /// eligieron allá después que aquí): hay que guardarlo.
    PeerLayout { id: EndpointId, side: Side, rev: u64 },
    /// El otro comparte su mouse y teclado con este equipo (lo dijo al
    /// conectarse), y ya se sabe de qué lado está: sirve para compartir de
    /// vuelta sin configurar nada.
    PeerShares { id: EndpointId, side: Side },
    /// El cursor (y el teclado) de este equipo pasó al otro.
    Away { id: EndpointId, name: String },
    /// Volvió a este equipo.
    Home,
    /// El mouse y el teclado del otro están (`on`) o dejaron de estar en este equipo.
    Controlled { id: EndpointId, name: String, on: bool },
    /// Llegó al portapapeles algo copiado en el otro equipo.
    Clipboard { id: EndpointId, name: String, image: bool },
    /// Empezaron a irse archivos al otro (copiados o arrastrados al borde).
    FilesSending { id: EndpointId, name: String, count: usize, bytes: u64, purpose: Purpose },
    /// Llegaron archivos del otro: `paths` son las raíces ya escritas aquí
    /// (en Descargas, o listas para pegar si se copiaron).
    FilesReceived { id: EndpointId, name: String, paths: Vec<PathBuf>, purpose: Purpose },
    /// Terminaron de irse.
    FilesSent { id: EndpointId, name: String, count: usize },
    FilesFailed { reason: String },
    /// Algo que el usuario debería saber (sin permiso para escuchar el teclado, por ejemplo).
    Error(String),
}

/// Lo que se copia y pasa de esto no se manda solo (se puede arrastrar al borde).
pub const MAX_CLIP_FILES: u64 = 200 * 1024 * 1024;

/// Cada cuánto se reintenta la conexión con el equipo con que se comparte.
const RETRY: Duration = Duration::from_secs(3);
/// Con el cursor en el otro, cada cuánto se le pregunta si sigue ahí, y
/// cuánto sin oír nada antes de devolverle el mouse y el teclado a este.
const PING_EVERY: Duration = Duration::from_secs(1);
const PING_LOST: Duration = Duration::from_secs(3);
/// Con qué se cierra la sesión que sobra o la del equipo que dejó de contestar.
const CLOSE_DUPLICATE: u32 = 4;
const CLOSE_SILENT: u32 = 5;

pub struct Service {
    shared: Arc<Shared>,
}

struct Shared {
    node: Arc<Node>,
    events: UnboundedSender<Event>,
    links: Mutex<HashMap<EndpointId, Link>>,
    next_link: AtomicU64,
    share: Mutex<Option<Share>>,
    connector: Mutex<Option<JoinHandle<()>>>,
    grab: OnceLock<Arc<Grab>>,
    clip: Arc<Clip>,
    clipboard: AtomicBool,
    /// De qué lado de este está cada equipo, como lo acordaron.
    layouts: Mutex<HashMap<EndpointId, Placement>>,
    /// Lo que dice un equipo de una versión anterior (que no puede adoptar el
    /// lado de aquí) cuando no coincide: de qué lado de este dice estar.
    claims: Mutex<HashMap<EndpointId, Side>>,
}

/// La sesión por la que se le mandan cosas a un equipo.
struct Link {
    id: u64,
    tx: UnboundedSender<Msg>,
    conn: Connection,
    /// La abrió este equipo.
    outgoing: bool,
    /// El otro entiende [`Msg::Layout`].
    layout2: bool,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Service {
    /// Abre la conexión con la identidad de `dir` y empieza a recibir. Hay que
    /// llamarlo dentro de un runtime de tokio.
    pub async fn start(dir: &Path, name: String, events: UnboundedSender<Event>) -> Result<Self> {
        sys::prepare();
        Ok(Self::run(Node::start(dir, name).await?, events))
    }

    /// Como [`Service::start`], con la identidad en el llavero del sistema
    /// ([`Node::start_in_keychain`]). Es lo que usa la app.
    pub async fn start_in_keychain(dir: &Path, name: String, events: UnboundedSender<Event>) -> Result<Self> {
        sys::prepare();
        Ok(Self::run(Node::start_in_keychain(dir, name).await?, events))
    }

    fn run(node: Arc<Node>, events: UnboundedSender<Event>) -> Self {
        let shared = Arc::new(Shared {
            node,
            events,
            links: Mutex::new(HashMap::new()),
            next_link: AtomicU64::new(1),
            share: Mutex::new(None),
            connector: Mutex::new(None),
            grab: OnceLock::new(),
            clip: Arc::new(Clip::default()),
            clipboard: AtomicBool::new(true),
            layouts: Mutex::new(HashMap::new()),
            claims: Mutex::new(HashMap::new()),
        });
        // Lo que llegó para pegar en una sesión anterior ya no hace falta.
        let _ = std::fs::remove_dir_all(shared.node.dir().join("clip"));
        tokio::spawn(accept_loop(shared.clone()));
        // Lo que se copia aquí va a todos los equipos conectados.
        let (clip_tx, mut clip_rx) = mpsc::unbounded_channel();
        shared.clip.watch(clip_tx);
        {
            let shared = shared.clone();
            tokio::spawn(async move {
                while let Some(copied) = clip_rx.recv().await {
                    if !shared.clipboard.load(Ordering::Relaxed) {
                        continue;
                    }
                    match copied {
                        Copied::Msg(msg) => {
                            for link in lock(&shared.links).values() {
                                let _ = link.tx.send(msg.clone());
                            }
                        }
                        Copied::Files(paths) => {
                            let peers: Vec<EndpointId> = lock(&shared.links).keys().copied().collect();
                            for peer in peers {
                                tokio::spawn(send_files(shared.clone(), peer, paths.clone(), Purpose::Clipboard));
                            }
                        }
                    }
                }
            });
        }
        // Si se conecta o se mueve un monitor.
        {
            let shared = shared.clone();
            tokio::spawn(async move {
                loop {
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    if let Some(grab) = shared.grab.get() {
                        grab.set_screens(sys::screens());
                    }
                }
            });
        }
        Self { shared }
    }

    pub fn id(&self) -> EndpointId {
        self.shared.node.id()
    }

    pub fn node(&self) -> &Arc<Node> {
        &self.shared.node
    }

    pub fn name(&self) -> &str {
        &self.shared.node.name
    }

    pub fn peers(&self) -> Vec<(EndpointId, Peer)> {
        self.shared.node.peers()
    }

    /// Los equipos vinculados con una sesión arriba.
    pub fn connected(&self) -> Vec<EndpointId> {
        lock(&self.shared.links).keys().copied().collect()
    }

    /// Los equipos con esto abierto en la red local (los ya vinculados incluidos).
    pub async fn nearby(&self, wait: Duration) -> Vec<(EndpointId, String)> {
        self.shared.node.nearby(wait).await
    }

    /// Abre el vínculo por `window`: solo mientras tanto se atiende a un equipo
    /// que pida vincularse con este (y se muestra su código). Se cierra solo
    /// al vincularse uno.
    pub fn open_pairing(&self, window: Duration) {
        self.shared.node.open_pairing(window);
    }

    pub fn close_pairing(&self) {
        self.shared.node.close_pairing();
    }

    pub fn pairing_open(&self) -> bool {
        self.shared.node.pairing_open()
    }

    /// Abierto, cerrado o bloqueado por intentos fallidos, y cuánto le queda.
    pub fn pairing_state(&self) -> PairingState {
        self.shared.node.pairing_state()
    }

    /// Pide vincularse con `id`; `code` es el código que muestra el otro.
    pub async fn pair(&self, id: EndpointId, code: impl Future<Output = Result<String>>) -> Result<Peer> {
        match self.shared.node.pair(id, code).await {
            Ok(peer) => {
                let _ = self.shared.events.send(Event::Paired { id, name: peer.name.clone() });
                Ok(peer)
            }
            Err(e) => {
                let _ = self.shared.events.send(Event::PairFailed { reason: format!("{e:#}") });
                Err(e)
            }
        }
    }

    /// Olvida un equipo y corta su sesión.
    pub fn forget(&self, id: &EndpointId) -> Result<()> {
        if lock(&self.shared.share).is_some_and(|s| s.peer == *id) {
            self.share(None)?;
        }
        self.shared.node.forget(id)?;
        lock(&self.shared.layouts).remove(id);
        lock(&self.shared.claims).remove(id);
        if let Some(link) = lock(&self.shared.links).remove(id) {
            link.conn.close(2u32.into(), b"olvidado");
        }
        Ok(())
    }

    /// Abre la sesión con un equipo vinculado aunque no se comparta el mouse
    /// con él (para mandarle archivos o el portapapeles).
    pub async fn connect(&self, peer: EndpointId) -> Result<()> {
        if lock(&self.shared.links).contains_key(&peer) {
            return Ok(());
        }
        let session = self.shared.node.connect(peer, hello(&self.shared, peer)).await?;
        tokio::spawn(run_session(self.shared.clone(), peer, session, true));
        for _ in 0..50 {
            if lock(&self.shared.links).contains_key(&peer) {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        anyhow::bail!("la sesión no quedó arriba")
    }

    /// Manda archivos o carpetas a un equipo conectado: quedan en sus Descargas.
    pub fn send_files(&self, peer: EndpointId, paths: Vec<PathBuf>) {
        tokio::spawn(send_files(self.shared.clone(), peer, paths, Purpose::Drop));
    }

    /// Compartir el portapapeles con los equipos conectados (prendido por defecto).
    pub fn set_clipboard(&self, on: bool) {
        self.shared.clipboard.store(on, Ordering::Relaxed);
    }

    pub fn sharing(&self) -> Option<Share> {
        *lock(&self.shared.share)
    }

    /// Lo guardado de una vez anterior (al arrancar): de qué lado está cada
    /// equipo. No pisa algo más nuevo que ya haya llegado del otro.
    pub fn set_placements(&self, saved: impl IntoIterator<Item = (EndpointId, Placement)>) {
        for (peer, placement) in saved {
            let newer = {
                let mut layouts = lock(&self.shared.layouts);
                let newer = layouts.get(&peer).is_none_or(|p| placement.rev > p.rev);
                if newer {
                    layouts.insert(peer, placement);
                }
                newer
            };
            if newer {
                follow_side(&self.shared, peer, placement.side);
                send_layout(&self.shared, peer, placement);
            }
        }
    }

    /// De qué lado de este está `peer`, si se eligió (aquí o allá).
    pub fn placement(&self, peer: &EndpointId) -> Option<Placement> {
        lock(&self.shared.layouts).get(peer).copied()
    }

    /// Lo que dice `peer` de qué lado de este está, cuando no coincide con lo
    /// de aquí y no se puede acordar (el otro tiene una versión anterior).
    pub fn claim(&self, peer: &EndpointId) -> Option<Side> {
        lock(&self.shared.claims).get(peer).copied()
    }

    /// El usuario eligió de qué lado de este está `peer`: si se comparte con
    /// él, el cursor cruza por ese borde, y se le avisa al otro para que
    /// quede igual allá. Devuelve lo que hay que guardar. No empieza a
    /// compartir.
    pub fn set_side(&self, peer: EndpointId, side: Side) -> Placement {
        let shared = &self.shared;
        let placement = {
            let mut layouts = lock(&shared.layouts);
            // Más nuevo que lo que ya se sabe, aunque el reloj del otro vaya adelantado.
            let rev = now_ms().max(layouts.get(&peer).map_or(0, |p| p.rev + 1));
            let placement = Placement { side, rev };
            layouts.insert(peer, placement);
            placement
        };
        lock(&shared.claims).remove(&peer);
        follow_side(shared, peer, side);
        send_layout(shared, peer, placement);
        placement
    }

    /// Comparte este mouse y teclado con un equipo vinculado (o deja de
    /// compartir). La primera vez instala el gancho del sistema: falla sin
    /// el permiso para escuchar el teclado y el mouse.
    pub fn share(&self, share: Option<Share>) -> Result<()> {
        let shared = &self.shared;
        if let Some(task) = lock(&shared.connector).take() {
            task.abort();
        }
        *lock(&shared.share) = share;
        // Compartir por un lado que no es el acordado es elegirlo.
        if let Some(share) = share {
            if lock(&shared.layouts).get(&share.peer).is_none_or(|p| p.side != share.side) {
                self.set_side(share.peer, share.side);
            }
        }
        let Some(share) = share else {
            if let Some(grab) = shared.grab.get() {
                grab.set_side(None);
                disconnect_grab(shared, grab);
            }
            return Ok(());
        };
        let grab = ensure_grab(shared)?;
        grab.set_side(Some(share.side));
        update_connected(shared);
        *lock(&shared.connector) = Some(tokio::spawn(connect_loop(shared.clone(), share.peer)));
        Ok(())
    }

    /// Antes de salir: el cursor vuelve a este equipo y deja de compartir.
    pub fn stop(&self) {
        let _ = self.share(None);
        for (_, link) in lock(&self.shared.links).drain() {
            link.conn.close(0u32.into(), b"");
        }
    }
}

/// El gancho del sistema y la tarea que manda lo que captura, una sola vez.
fn ensure_grab(shared: &Arc<Shared>) -> Result<Arc<Grab>> {
    if let Some(grab) = shared.grab.get() {
        return Ok(grab.clone());
    }
    let (tx, mut rx) = mpsc::unbounded_channel();
    let grab = Arc::new(Grab::new(None, sys::screens(), tx));
    if let Err(e) = sys::start_capture(grab.clone()) {
        let _ = shared.events.send(Event::Error(format!("{e:#}")));
        return Err(e);
    }
    let _ = shared.grab.set(grab.clone());
    // Un arrastre de archivos que llega al borde: los archivos van a Descargas del otro.
    {
        let weak = Arc::downgrade(shared);
        let runtime = tokio::runtime::Handle::current();
        grab.on_drag_edge(move || {
            let Some(shared) = weak.upgrade() else { return };
            runtime.spawn(async move {
                let Some(share) = *lock(&shared.share) else { return };
                let paths = tokio::task::spawn_blocking(sys::dragged_files).await.unwrap_or_default();
                if !paths.is_empty() {
                    send_files(shared, share.peer, paths, Purpose::Drop).await;
                }
            });
        });
    }
    let pump = shared.clone();
    tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            let Some(share) = *lock(&pump.share) else { continue };
            match &msg {
                Msg::Enter { .. } => {
                    let name = pump.node.peer(&share.peer).map(|p| p.name).unwrap_or_default();
                    let _ = pump.events.send(Event::Away { id: share.peer, name });
                }
                Msg::Leave => {
                    let _ = pump.events.send(Event::Home);
                }
                _ => {}
            }
            if let Some(link) = lock(&pump.links).get(&share.peer) {
                let _ = link.tx.send(msg);
            }
        }
    });
    Ok(grab)
}

/// El cursor solo cruza si hay sesión con el equipo con que se comparte.
fn update_connected(shared: &Shared) {
    let Some(grab) = shared.grab.get() else { return };
    let up = lock(&shared.share).is_some_and(|s| lock(&shared.links).contains_key(&s.peer));
    if up {
        grab.set_connected(true);
    } else {
        disconnect_grab(shared, grab);
    }
}

/// Sin sesión el cursor no cruza; si estaba en el otro, vuelve (y se avisa).
fn disconnect_grab(shared: &Shared, grab: &Grab) {
    let was_away = grab.is_remote();
    grab.set_connected(false);
    if was_away {
        let _ = shared.events.send(Event::Home);
    }
}

fn hello(shared: &Shared, peer: EndpointId) -> Msg {
    // De qué lado del otro está este equipo, si comparte con él (las versiones
    // anteriores lo leen como «comparte conmigo»: solo entonces).
    let layout = lock(&shared.share).filter(|s| s.peer == peer).map(|s| s.side.opposite());
    // Y siempre que haya un lado elegido, con cuándo se eligió.
    let place = lock(&shared.layouts).get(&peer).copied();
    Msg::Hello {
        name: shared.node.name.clone(),
        os: Os::current(),
        screens: sys::screens().0,
        layout,
        caps: caps(),
        place: place.map(|p| p.side.opposite()),
        layout_rev: place.map_or(0, |p| p.rev),
    }
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

/// Si lo que dice el otro (`theirs`, ya visto desde este equipo) gana a lo de
/// aquí: el cambio más nuevo y, si son del mismo momento, el del equipo de id
/// mayor. Los dos lados llegan a lo mismo.
fn theirs_wins(ours: Option<Placement>, theirs: Placement, own: EndpointId, peer: EndpointId) -> bool {
    match ours {
        None => true,
        Some(ours) if ours == theirs => false,
        Some(ours) => theirs.rev > ours.rev || (theirs.rev == ours.rev && peer > own),
    }
}

/// Si se comparte con `peer`, el cursor sigue cruzando por `side`.
fn follow_side(shared: &Shared, peer: EndpointId, side: Side) {
    let mut share = lock(&shared.share);
    if let Some(share) = share.as_mut().filter(|s| s.peer == peer) {
        share.side = side;
        if let Some(grab) = shared.grab.get() {
            grab.set_side(Some(side));
        }
    }
}

/// Le dice a `peer` de qué lado de él está este equipo, si lo entiende.
fn send_layout(shared: &Shared, peer: EndpointId, placement: Placement) {
    if let Some(link) = lock(&shared.links).get(&peer).filter(|l| l.layout2) {
        let _ = link.tx.send(Msg::Layout { side: placement.side.opposite(), rev: placement.rev });
    }
}

/// Lo que dice `peer` de qué lado de este está. `answers`: entiende
/// [`Msg::Layout`] (puede adoptar el de aquí).
fn heard_layout(shared: &Shared, peer: EndpointId, theirs: Placement, answers: bool) {
    let ours = {
        let mut layouts = lock(&shared.layouts);
        let ours = layouts.get(&peer).copied();
        if theirs_wins(ours, theirs, shared.node.id(), peer) {
            layouts.insert(peer, theirs);
            None
        } else {
            Some(ours)
        }
    };
    let Some(ours) = ours else {
        lock(&shared.claims).remove(&peer);
        follow_side(shared, peer, theirs.side);
        let _ = shared.events.send(Event::PeerLayout { id: peer, side: theirs.side, rev: theirs.rev });
        return;
    };
    let Some(ours) = ours.filter(|ours| ours.side != theirs.side) else {
        lock(&shared.claims).remove(&peer);
        return;
    };
    if answers {
        // Gana el de aquí: se le repite, por si no lo tenía.
        send_layout(shared, peer, ours);
    } else {
        lock(&shared.claims).insert(peer, theirs.side);
    }
}

/// Entre dos sesiones con el mismo equipo, si la nueva reemplaza a la que
/// había: se queda la que abrió el de id mayor (los dos lados eligen la
/// misma). Si las abrió el mismo, la nueva: la otra puede estar muerta sin
/// saberlo todavía.
fn replaces(new_outgoing: bool, old_outgoing: bool, own: EndpointId, peer: EndpointId) -> bool {
    new_outgoing == old_outgoing || new_outgoing == (own > peer)
}

/// Mantiene arriba la sesión con el equipo con que se comparte.
async fn connect_loop(shared: Arc<Shared>, peer: EndpointId) {
    loop {
        if !lock(&shared.links).contains_key(&peer) {
            match shared.node.connect(peer, hello(&shared, peer)).await {
                Ok(session) => run_session(shared.clone(), peer, session, true).await,
                Err(e) => tracing::debug!("sin conexión con {}: {e:#}", peer.fmt_short()),
            }
        }
        tokio::time::sleep(RETRY).await;
    }
}

async fn accept_loop(shared: Arc<Shared>) {
    while let Some(incoming) = shared.node.endpoint.accept().await {
        let shared = shared.clone();
        tokio::spawn(async move {
            let conn = match incoming.accept() {
                Ok(accepting) => match accepting.await {
                    Ok(conn) => conn,
                    Err(e) => return tracing::debug!("conexión fallida: {e:#}"),
                },
                Err(e) => return tracing::debug!("conexión rechazada: {e:#}"),
            };
            let id = conn.remote_id();
            if conn.alpn() == PAIR_ALPN {
                let events = shared.events.clone();
                let show = move |name: &str, code: &str| {
                    let _ = events.send(Event::PairCode { name: name.into(), code: code.into() });
                };
                let event = match shared.node.serve_pair(conn, show).await {
                    Ok(Some(peer)) => Event::Paired { id, name: peer.name },
                    // No se atendió (el vínculo no está abierto): sin aviso, para
                    // que un desconocido no pueda llenar la pantalla de errores.
                    Ok(None) => return tracing::debug!("pedido de vínculo de {} sin atender", id.fmt_short()),
                    Err(e) => Event::PairFailed { reason: format!("{e:#}") },
                };
                let _ = shared.events.send(event);
                return;
            }
            match shared.node.accept_session(conn, hello(&shared, id)).await {
                Ok(session) => run_session(shared, id, session, false).await,
                Err(e) => tracing::debug!("{e:#}"),
            }
        });
    }
}

/// Atiende una sesión hasta que se corta: inyecta lo que llega, devuelve el
/// cursor, escribe el portapapeles del otro y manda lo que toque por ella.
/// `outgoing`: la abrió este equipo.
async fn run_session(shared: Arc<Shared>, peer: EndpointId, session: Session, outgoing: bool) {
    let Session { conn, mut send, mut recv, hello } = session;
    let Msg::Hello { name, os, layout, caps, place, layout_rev, .. } = hello else { return };
    let name = clean_name(&name);
    let layout2 = caps.iter().any(|c| c == CAP_LAYOUT2);
    let ping = caps.iter().any(|c| c == CAP_PING);
    let (tx, mut rx) = mpsc::unbounded_channel::<Msg>();
    let link_id = shared.next_link.fetch_add(1, Ordering::Relaxed);
    let link = Link { id: link_id, tx: tx.clone(), conn: conn.clone(), outgoing, layout2 };
    // `None`: sobra (ya hay una mejor). `Some(old)`: queda esta; `old`, la que reemplazó.
    let registered = {
        let mut links = lock(&shared.links);
        let keep = links.get(&peer).is_none_or(|old| replaces(outgoing, old.outgoing, shared.node.id(), peer));
        keep.then(|| links.insert(peer, link))
    };
    let Some(replaced) = registered else {
        tracing::debug!("sesión repetida con {name}: se cierra");
        conn.close(CLOSE_DUPLICATE.into(), b"repetida");
        return;
    };
    match replaced {
        Some(old) => old.conn.close(CLOSE_DUPLICATE.into(), b"repetida"),
        None => {
            let _ = shared.events.send(Event::Connected { id: peer, name: name.clone() });
            update_connected(&shared);
        }
    }
    // De qué lado dice estar. Una versión anterior solo lo dice si comparte.
    let claim = if layout2 { place.map(|side| Placement { side, rev: layout_rev }) } else { layout.map(|side| Placement { side, rev: 0 }) };
    if let Some(theirs) = claim {
        heard_layout(&shared, peer, theirs, layout2);
    }
    if layout.is_some() {
        if let Some(placement) = lock(&shared.layouts).get(&peer).copied() {
            let _ = shared.events.send(Event::PeerShares { id: peer, side: placement.side });
        }
    }
    // Lo último que se oyó del otro (cualquier mensaje).
    let heard = Arc::new(Mutex::new(Instant::now()));
    let pinger = ping.then(|| tokio::spawn(keep_alive(shared.clone(), peer, link_id, tx.clone(), heard.clone(), conn.clone())));
    let writer = {
        let conn = conn.clone();
        tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                if write_frame(&mut send, &msg).await.is_err() {
                    break;
                }
            }
            conn.close(0u32.into(), b"");
        })
    };
    // Los archivos llegan por corrientes aparte.
    let receiver = tokio::spawn(receive_files(shared.clone(), peer, name.clone(), conn.clone()));
    let mut target = Target::new(os);
    // El mouse del otro está en este equipo.
    let mut controlled = false;
    loop {
        let incoming = match read_msg(&mut recv).await {
            Ok(Some(incoming)) => incoming,
            Ok(None) => break,
            Err(e) => {
                tracing::debug!("la sesión con {name} terminó: {e:#}");
                break;
            }
        };
        *lock(&heard) = Instant::now();
        // De una versión más nueva del otro: se salta.
        let Incoming::Msg(msg) = incoming else { continue };
        match msg {
            Msg::Layout { side, rev } => heard_layout(&shared, peer, Placement { side, rev }, true),
            Msg::Ping => {
                let _ = tx.send(Msg::Pong);
            }
            Msg::Pong => {}
            Msg::Back { along } => {
                if let Some(grab) = shared.grab.get() {
                    if grab.is_remote() {
                        grab.back(along);
                        let _ = shared.events.send(Event::Home);
                    }
                }
            }
            Msg::ClipText { .. } | Msg::ClipImage { .. } => {
                if shared.clipboard.load(Ordering::Relaxed) {
                    let image = matches!(msg, Msg::ClipImage { .. });
                    let _ = shared.events.send(Event::Clipboard { id: peer, name: name.clone(), image });
                    let clip = shared.clip.clone();
                    tokio::task::spawn_blocking(move || {
                        if let Err(e) = clip.apply(&msg) {
                            tracing::warn!("no se pudo escribir el portapapeles: {e:#}");
                        }
                    });
                }
            }
            Msg::Hello { .. } => {}
            msg => {
                let entering = matches!(msg, Msg::Enter { .. });
                let leaving = matches!(msg, Msg::Leave);
                let reply = target.handle(msg);
                let gone = leaving || matches!(reply, Some(Msg::Back { .. }));
                if entering != controlled && (entering || gone) {
                    controlled = entering;
                    let _ = shared.events.send(Event::Controlled { id: peer, name: name.clone(), on: controlled });
                }
                if let Some(reply) = reply {
                    let _ = tx.send(reply);
                }
            }
        }
    }
    target.release_all();
    receiver.abort();
    if let Some(pinger) = pinger {
        pinger.abort();
    }
    if controlled {
        let _ = shared.events.send(Event::Controlled { id: peer, name: name.clone(), on: false });
    }
    writer.abort();
    conn.close(0u32.into(), b"");
    let removed = {
        let mut links = lock(&shared.links);
        let mine = links.get(&peer).is_some_and(|l| l.id == link_id);
        if mine {
            links.remove(&peer);
        }
        mine
    };
    if removed {
        update_connected(&shared);
        let _ = shared.events.send(Event::Disconnected { id: peer, name });
    }
}

/// Con el cursor de este equipo en `peer`, le pregunta cada segundo si sigue
/// ahí. Si deja de contestar (se colgó, se durmió o se cortó la red), se
/// cierra la sesión: el mouse y el teclado vuelven a este equipo enseguida en
/// vez de quedar tragados hasta que QUIC note el corte, y `connect_loop`
/// vuelve a conectar.
async fn keep_alive(shared: Arc<Shared>, peer: EndpointId, link_id: u64, tx: UnboundedSender<Msg>, heard: Arc<Mutex<Instant>>, conn: Connection) {
    let mut asking = false;
    loop {
        tokio::time::sleep(PING_EVERY).await;
        let away = shared.grab.get().is_some_and(|g| g.is_remote())
            && lock(&shared.share).is_some_and(|s| s.peer == peer)
            && lock(&shared.links).get(&peer).is_some_and(|l| l.id == link_id);
        if !away {
            asking = false;
            continue;
        }
        if !asking {
            // Se cuenta desde que se empieza a preguntar.
            asking = true;
            *lock(&heard) = Instant::now();
        } else if lock(&heard).elapsed() > PING_LOST {
            tracing::info!("{} no contesta: el cursor vuelve", peer.fmt_short());
            conn.close(CLOSE_SILENT.into(), b"sin respuesta");
            return;
        }
        let _ = tx.send(Msg::Ping);
    }
}

/// Manda archivos a `peer` por su sesión. Los copiados que pasan de
/// [`MAX_CLIP_FILES`] no se mandan (se avisa).
async fn send_files(shared: Arc<Shared>, peer: EndpointId, paths: Vec<PathBuf>, purpose: Purpose) {
    let Some(conn) = lock(&shared.links).get(&peer).map(|l| l.conn.clone()) else { return };
    let name = shared.node.peer(&peer).map(|p| p.name).unwrap_or_default();
    let listed = tokio::task::spawn_blocking(move || files::collect(&paths, purpose)).await;
    let (offer, sources) = match listed {
        Ok(Ok(listed)) => listed,
        Ok(Err(e)) => return fail(&shared, format!("{e:#}")),
        Err(e) => return fail(&shared, e.to_string()),
    };
    let bytes = offer.total();
    if purpose == Purpose::Clipboard && bytes > MAX_CLIP_FILES {
        tracing::info!("lo copiado pesa {bytes} bytes: no se manda solo");
        return;
    }
    let count = offer.entries.iter().filter(|e| !e.dir).count();
    let _ = shared.events.send(Event::FilesSending { id: peer, name: name.clone(), count, bytes, purpose });
    match files::send(&conn, &offer, &sources).await {
        Ok(()) => {
            let _ = shared.events.send(Event::FilesSent { id: peer, name, count });
        }
        Err(e) => fail(&shared, format!("{e:#}")),
    }
}

fn fail(shared: &Shared, reason: String) {
    tracing::warn!("archivos: {reason}");
    let _ = shared.events.send(Event::FilesFailed { reason });
}

/// Atiende los envíos de archivos del otro mientras dura la sesión.
async fn receive_files(shared: Arc<Shared>, peer: EndpointId, name: String, conn: Connection) {
    while let Ok(stream) = conn.accept_uni().await {
        let shared = shared.clone();
        let name = name.clone();
        tokio::spawn(async move {
            let clip_dir = shared.node.dir().join("clip").join(crate::files::stamp());
            let dest = move |purpose: Purpose| match purpose {
                Purpose::Clipboard => Ok(clip_dir),
                Purpose::Drop => files::downloads_dir(),
            };
            match files::receive(stream, dest).await {
                Ok((purpose, paths)) => {
                    if purpose == Purpose::Clipboard && shared.clipboard.load(Ordering::Relaxed) {
                        let clip = shared.clip.clone();
                        let roots = paths.clone();
                        let _ = tokio::task::spawn_blocking(move || clip.set_files(&roots)).await;
                    }
                    let _ = shared.events.send(Event::FilesReceived { id: peer, name, paths, purpose });
                }
                Err(e) => fail(&shared, format!("{e:#}")),
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn next(rx: &mut mpsc::UnboundedReceiver<Event>, what: impl Fn(&Event) -> bool) -> Event {
        loop {
            let event = tokio::time::timeout(Duration::from_secs(20), rx.recv()).await.expect("no llegó a tiempo").unwrap();
            if what(&event) {
                return event;
            }
        }
    }

    /// El PC se vincula con el Mac y comparte con él (a su derecha): los dos
    /// quedan conectados y el Mac se entera de que el PC está a su izquierda.
    #[tokio::test]
    async fn compartir_conecta_y_avisa_el_lado() {
        let base = std::env::temp_dir().join(format!("atic-kvm-service-{}", std::process::id()));
        let (pc_tx, mut pc_rx) = mpsc::unbounded_channel();
        let (mac_tx, mut mac_rx) = mpsc::unbounded_channel();
        let pc = Service::start(&base.join("pc"), "PC".into(), pc_tx).await.unwrap();
        let mac = Service::start(&base.join("mac"), "Mac".into(), mac_tx).await.unwrap();
        let mac_addr = mac.node().endpoint.addr();
        pc.node().endpoint.address_lookup().unwrap().add(iroh::address_lookup::MemoryLookup::from_endpoint_info([mac_addr]));

        // Sin abrir el vínculo en el Mac, el pedido se rechaza y allá no aparece nada.
        let closed = pc.pair(mac.id(), std::future::pending()).await.unwrap_err();
        assert!(closed.to_string().contains("no está esperando"), "{closed:#}");
        assert!(mac.peers().is_empty());
        assert!(mac_rx.try_recv().is_err());

        // El código lo muestra el Mac; lo "escribe" el PC.
        mac.open_pairing(Duration::from_secs(60));
        let (code_tx, code_rx) = tokio::sync::oneshot::channel::<String>();
        let shown = tokio::spawn(async move {
            let Event::PairCode { code, .. } = next(&mut mac_rx, |e| matches!(e, Event::PairCode { .. })).await else { unreachable!() };
            code_tx.send(code).unwrap();
            mac_rx
        });
        let peer = pc.pair(mac.id(), async move { Ok(code_rx.await?) }).await.unwrap();
        assert_eq!(peer.name, "Mac");
        let mut mac_rx = shown.await.unwrap();
        next(&mut mac_rx, |e| matches!(e, Event::Paired { .. })).await;

        pc.share(Some(Share { peer: mac.id(), side: Side::Right })).unwrap();
        next(&mut pc_rx, |e| matches!(e, Event::Connected { .. })).await;
        let Event::PeerLayout { id, side, .. } = next(&mut mac_rx, |e| matches!(e, Event::PeerLayout { .. })).await else { unreachable!() };
        assert_eq!((id, side), (pc.id(), Side::Left));
        assert_eq!(pc.connected(), vec![mac.id()]);

        // Una carpeta arrastrada al borde llega entera a las Descargas del otro.
        let downloads = base.join("descargas");
        std::env::set_var("ATIC_KVM_DOWNLOADS", &downloads);
        let folder = base.join("Fotos");
        std::fs::create_dir_all(folder.join("viaje")).unwrap();
        std::fs::write(folder.join("viaje/playa.jpg"), vec![7u8; 300_000]).unwrap();
        std::fs::write(folder.join("nota.txt"), "hola ñ").unwrap();
        pc.send_files(mac.id(), vec![folder]);
        next(&mut pc_rx, |e| matches!(e, Event::FilesSent { count: 2, .. })).await;
        let Event::FilesReceived { paths, purpose, .. } = next(&mut mac_rx, |e| matches!(e, Event::FilesReceived { .. })).await else { unreachable!() };
        assert_eq!(purpose, Purpose::Drop);
        assert_eq!(paths, vec![downloads.join("Fotos")]);
        assert_eq!(std::fs::read(downloads.join("Fotos/viaje/playa.jpg")).unwrap(), vec![7u8; 300_000]);
        assert_eq!(std::fs::read_to_string(downloads.join("Fotos/nota.txt")).unwrap(), "hola ñ");
        // Vienen de afuera: quedan marcados como lo que baja el navegador.
        #[cfg(windows)]
        for file in ["Fotos/nota.txt", "Fotos/viaje/playa.jpg"] {
            let mut zone = downloads.join(file).into_os_string();
            zone.push(":Zone.Identifier");
            assert!(std::fs::read_to_string(zone).unwrap().contains("ZoneId=3"), "{file}");
        }

        // Dejar de compartir no corta la sesión (el portapapeles sigue); olvidar sí.
        pc.share(None).unwrap();
        assert_eq!(pc.sharing(), None);
        pc.forget(&mac.id()).unwrap();
        assert!(pc.peers().is_empty());
        next(&mut mac_rx, |e| matches!(e, Event::Disconnected { .. })).await;
        let _ = std::fs::remove_dir_all(base);
    }

    type Rx = mpsc::UnboundedReceiver<Event>;

    /// Dos servicios en el mismo proceso, ya vinculados (el PC encuentra al
    /// Mac sin depender del mDNS de la red).
    async fn vinculados(base: &Path) -> (Service, Rx, Service, Rx) {
        let (pc_tx, pc_rx) = mpsc::unbounded_channel();
        let (mac_tx, mut mac_rx) = mpsc::unbounded_channel();
        let pc = Service::start(&base.join("pc"), "PC".into(), pc_tx).await.unwrap();
        let mac = Service::start(&base.join("mac"), "Mac".into(), mac_tx).await.unwrap();
        pc.node().endpoint.address_lookup().unwrap().add(iroh::address_lookup::MemoryLookup::from_endpoint_info([mac.node().endpoint.addr()]));
        mac.node().endpoint.address_lookup().unwrap().add(iroh::address_lookup::MemoryLookup::from_endpoint_info([pc.node().endpoint.addr()]));
        mac.open_pairing(Duration::from_secs(60));
        let (code_tx, code_rx) = tokio::sync::oneshot::channel::<String>();
        let shown = tokio::spawn(async move {
            let Event::PairCode { code, .. } = next(&mut mac_rx, |e| matches!(e, Event::PairCode { .. })).await else { unreachable!() };
            code_tx.send(code).unwrap();
            next(&mut mac_rx, |e| matches!(e, Event::Paired { .. })).await;
            mac_rx
        });
        pc.pair(mac.id(), async move { Ok(code_rx.await?) }).await.unwrap();
        let mac_rx = shown.await.unwrap();
        (pc, pc_rx, mac, mac_rx)
    }

    /// El lado que eligió cada uno, visto desde el otro, es el contrario.
    fn agree(pc: &Service, mac: &Service) -> (Side, Side) {
        let from_pc = pc.placement(&mac.id()).unwrap();
        let from_mac = mac.placement(&pc.id()).unwrap();
        assert_eq!(from_pc.side, from_mac.side.opposite(), "no quedaron de acuerdo");
        assert_eq!(from_pc.rev, from_mac.rev);
        (from_pc.side, from_mac.side)
    }

    /// Los dos eligieron lados que no calzan (los dos dicen que el otro está a
    /// su derecha) en el mismo momento: al conectarse gana el de id mayor y el
    /// otro lo adopta. Después, un cambio en uno llega al otro sin compartir.
    #[tokio::test]
    async fn los_lados_se_acuerdan_y_el_empate_lo_gana_el_id_mayor() {
        let base = std::env::temp_dir().join(format!("atic-kvm-lados-{}", std::process::id()));
        let (pc, mut pc_rx, mac, mut mac_rx) = vinculados(&base).await;
        let same = Placement { side: Side::Right, rev: 100 };
        pc.set_placements([(mac.id(), same)]);
        mac.set_placements([(pc.id(), same)]);
        let pc_wins = pc.id() > mac.id();

        pc.connect(mac.id()).await.unwrap();
        let loser = if pc_wins { &mut mac_rx } else { &mut pc_rx };
        let Event::PeerLayout { side, rev, .. } = next(loser, |e| matches!(e, Event::PeerLayout { .. })).await else { unreachable!() };
        assert_eq!((side, rev), (Side::Left, 100));
        let (from_pc, from_mac) = agree(&pc, &mac);
        if pc_wins {
            assert_eq!(from_pc, Side::Right);
        } else {
            assert_eq!(from_mac, Side::Right);
        }
        // Nadie empezó a compartir por elegir un lado.
        assert_eq!((pc.sharing(), mac.sharing()), (None, None));

        // Con la sesión arriba también en el Mac, el Mac pone al PC arriba: el PC queda abajo del Mac.
        for _ in 0..250 {
            if mac.connected().contains(&pc.id()) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let chosen = mac.set_side(pc.id(), Side::Top);
        assert!(chosen.rev > 100);
        let Event::PeerLayout { id, side, rev } = next(&mut pc_rx, |e| matches!(e, Event::PeerLayout { .. })).await else { unreachable!() };
        assert_eq!((id, side, rev), (mac.id(), Side::Bottom, chosen.rev));
        assert_eq!(agree(&pc, &mac), (Side::Bottom, Side::Top));
        assert_eq!(pc.claim(&mac.id()), None);

        // Lo guardado más viejo que lo acordado no lo pisa.
        pc.set_placements([(mac.id(), same)]);
        assert_eq!(pc.placement(&mac.id()).unwrap().side, Side::Bottom);
        pc.stop();
        mac.stop();
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn gana_el_cambio_mas_nuevo_y_con_empate_el_id_mayor() {
        let low = EndpointId::from_bytes(&{
            let mut one = [0u8; 32];
            one[0] = 1;
            one
        })
        .unwrap();
        let high = EndpointId::from_bytes(&{
            let mut base = [0x66u8; 32];
            base[0] = 0x58;
            base
        })
        .unwrap();
        let (own, peer) = if low < high { (low, high) } else { (high, low) };
        let at = |side, rev| Placement { side, rev };
        // Sin nada aquí, lo del otro.
        assert!(theirs_wins(None, at(Side::Left, 0), own, peer));
        // Lo mismo no cambia nada.
        assert!(!theirs_wins(Some(at(Side::Left, 5)), at(Side::Left, 5), own, peer));
        // El más nuevo.
        assert!(theirs_wins(Some(at(Side::Left, 5)), at(Side::Top, 6), own, peer));
        assert!(!theirs_wins(Some(at(Side::Left, 6)), at(Side::Top, 5), own, peer));
        // Empate: el id mayor, y los dos lados llegan a lo mismo.
        assert!(theirs_wins(Some(at(Side::Left, 5)), at(Side::Top, 5), own, peer));
        assert!(!theirs_wins(Some(at(Side::Top, 5)), at(Side::Left, 5), peer, own));
    }

    #[test]
    fn la_sesion_que_queda_es_la_que_abrio_el_id_mayor() {
        let (a, b) = {
            let mut one = [0u8; 32];
            one[0] = 1;
            let mut base = [0x66u8; 32];
            base[0] = 0x58;
            let (x, y) = (EndpointId::from_bytes(&one).unwrap(), EndpointId::from_bytes(&base).unwrap());
            if x > y { (x, y) } else { (y, x) }
        };
        // `a` es el mayor: en `a` gana la que abrió (saliente); en `b`, la que recibió.
        assert!(replaces(true, false, a, b));
        assert!(!replaces(false, true, a, b));
        assert!(replaces(false, true, b, a));
        assert!(!replaces(true, false, b, a));
        // Dos abiertas por el mismo: la nueva.
        assert!(replaces(true, true, a, b) && replaces(false, false, b, a));
    }

    /// `share.json` de antes guardaba solo el lado.
    #[test]
    fn el_lado_guardado_antes_se_lee_con_rev_cero() {
        let old: Placement = serde_json::from_str(r#""top""#).unwrap();
        assert_eq!(old, Placement { side: Side::Top, rev: 0 });
        let new: Placement = serde_json::from_str(r#"{"side":"left","rev":42}"#).unwrap();
        assert_eq!(new, Placement { side: Side::Left, rev: 42 });
        assert_eq!(serde_json::to_string(&new).unwrap(), r#"{"side":"left","rev":42}"#);
    }
}
