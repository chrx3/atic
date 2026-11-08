//! Todo junto, como lo usa una app (la pill o el ejecutable de prueba): un
//! [`Service`] que siempre recibe (el mouse y el teclado de los equipos
//! vinculados pueden pasar a este) y, si se le dice, comparte el de aquí
//! con uno de ellos ([`Service::share`]). Los dos sentidos a la vez.
//!
//! Por cada equipo vinculado hay a lo más una sesión que se usa para
//! mandarle cosas ([`Link`]): la abre quien comparte, y si los dos comparten
//! se usa la primera que quedó arriba. Por ella van el mouse y el teclado de
//! aquí, las respuestas (devolver el cursor) y el portapapeles.
//!
//! Lo que pasa se avisa por [`Event`]s.

use std::collections::HashMap;
use std::future::Future;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::Duration;

use anyhow::Result;
use iroh::endpoint::Connection;
use iroh::EndpointId;
use tokio::sync::mpsc::{self, UnboundedSender};
use tokio::task::JoinHandle;

use crate::clip::Clip;
use crate::layout::Side;
use crate::link::{Node, Peer, Session, PAIR_ALPN};
use crate::proto::{read_frame, write_frame, Msg, Os};
use crate::source::Grab;
use crate::sys;
use crate::target::Target;

/// Con qué equipo se comparte este mouse y teclado, y de qué lado de este está.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Share {
    pub peer: EndpointId,
    pub side: Side,
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
    /// El otro comparte su mouse y teclado con este equipo y dice de qué lado
    /// de este está: sirve para compartir de vuelta sin configurar nada.
    PeerLayout { id: EndpointId, side: Side },
    /// Algo que el usuario debería saber (sin permiso para escuchar el teclado, por ejemplo).
    Error(String),
}

/// Cada cuánto se reintenta la conexión con el equipo con que se comparte.
const RETRY: Duration = Duration::from_secs(3);

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
}

/// La sesión por la que se le mandan cosas a un equipo.
struct Link {
    id: u64,
    tx: UnboundedSender<Msg>,
    conn: Connection,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Service {
    /// Abre la conexión con la identidad de `dir` y empieza a recibir. Hay que
    /// llamarlo dentro de un runtime de tokio.
    pub async fn start(dir: &Path, name: String, events: UnboundedSender<Event>) -> Result<Self> {
        sys::prepare();
        let node = Node::start(dir, name).await?;
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
        });
        tokio::spawn(accept_loop(shared.clone()));
        // Lo que se copia aquí va a todos los equipos conectados.
        let (clip_tx, mut clip_rx) = mpsc::unbounded_channel();
        shared.clip.watch(clip_tx);
        {
            let shared = shared.clone();
            tokio::spawn(async move {
                while let Some(msg) = clip_rx.recv().await {
                    if shared.clipboard.load(Ordering::Relaxed) {
                        for link in lock(&shared.links).values() {
                            let _ = link.tx.send(msg.clone());
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
        Ok(Self { shared })
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
        if let Some(link) = lock(&self.shared.links).remove(id) {
            link.conn.close(2u32.into(), b"olvidado");
        }
        Ok(())
    }

    /// Compartir el portapapeles con los equipos conectados (prendido por defecto).
    pub fn set_clipboard(&self, on: bool) {
        self.shared.clipboard.store(on, Ordering::Relaxed);
    }

    pub fn sharing(&self) -> Option<Share> {
        *lock(&self.shared.share)
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
        let Some(share) = share else {
            if let Some(grab) = shared.grab.get() {
                grab.set_side(None);
                grab.set_connected(false);
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
    let pump = shared.clone();
    tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            let Some(share) = *lock(&pump.share) else { continue };
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
    grab.set_connected(up);
}

fn hello(shared: &Shared, peer: EndpointId) -> Msg {
    // De qué lado del otro está este equipo, si comparte con él.
    let layout = lock(&shared.share).filter(|s| s.peer == peer).map(|s| s.side.opposite());
    Msg::Hello { name: shared.node.name.clone(), os: Os::current(), screens: sys::screens().0, layout }
}

/// Mantiene arriba la sesión con el equipo con que se comparte.
async fn connect_loop(shared: Arc<Shared>, peer: EndpointId) {
    loop {
        if !lock(&shared.links).contains_key(&peer) {
            match shared.node.connect(peer, hello(&shared, peer)).await {
                Ok(session) => run_session(shared.clone(), peer, session).await,
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
                    Ok(peer) => Event::Paired { id, name: peer.name },
                    Err(e) => Event::PairFailed { reason: format!("{e:#}") },
                };
                let _ = shared.events.send(event);
                return;
            }
            match shared.node.accept_session(conn, hello(&shared, id)).await {
                Ok(session) => run_session(shared, id, session).await,
                Err(e) => tracing::debug!("{e:#}"),
            }
        });
    }
}

/// Atiende una sesión hasta que se corta: inyecta lo que llega, devuelve el
/// cursor, escribe el portapapeles del otro y manda lo que toque por ella.
async fn run_session(shared: Arc<Shared>, peer: EndpointId, session: Session) {
    let Session { conn, mut send, mut recv, hello } = session;
    let Msg::Hello { name, os, layout, .. } = hello else { return };
    let (tx, mut rx) = mpsc::unbounded_channel::<Msg>();
    let link_id = shared.next_link.fetch_add(1, Ordering::Relaxed);
    let registered = {
        let mut links = lock(&shared.links);
        let vacant = !links.contains_key(&peer);
        if vacant {
            links.insert(peer, Link { id: link_id, tx: tx.clone(), conn: conn.clone() });
        }
        vacant
    };
    if registered {
        let _ = shared.events.send(Event::Connected { id: peer, name: name.clone() });
        update_connected(&shared);
    }
    if let Some(side) = layout {
        let _ = shared.events.send(Event::PeerLayout { id: peer, side });
    }
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
    let mut target = Target::new(os);
    loop {
        let msg = match read_frame::<_, Msg>(&mut recv).await {
            Ok(Some(msg)) => msg,
            Ok(None) => break,
            Err(e) => {
                tracing::debug!("la sesión con {name} terminó: {e:#}");
                break;
            }
        };
        match msg {
            Msg::Back { along } => {
                if let Some(grab) = shared.grab.get() {
                    grab.back(along);
                }
            }
            Msg::ClipText { .. } | Msg::ClipImage { .. } => {
                if shared.clipboard.load(Ordering::Relaxed) {
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
                if let Some(reply) = target.handle(msg) {
                    let _ = tx.send(reply);
                }
            }
        }
    }
    target.release_all();
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

        // El código lo muestra el Mac; lo "escribe" el PC.
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
        let Event::PeerLayout { id, side } = next(&mut mac_rx, |e| matches!(e, Event::PeerLayout { .. })).await else { unreachable!() };
        assert_eq!((id, side), (pc.id(), Side::Left));
        assert_eq!(pc.connected(), vec![mac.id()]);

        // Dejar de compartir no corta la sesión (el portapapeles sigue); olvidar sí.
        pc.share(None).unwrap();
        assert_eq!(pc.sharing(), None);
        pc.forget(&mac.id()).unwrap();
        assert!(pc.peers().is_empty());
        next(&mut mac_rx, |e| matches!(e, Event::Disconnected { .. })).await;
        let _ = std::fs::remove_dir_all(base);
    }
}
