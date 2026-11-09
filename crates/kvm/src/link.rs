//! La conexión entre los dos computadores, sobre iroh: QUIC cifrado de punta
//! a punta, directo en la red local (o por relay si no hay camino directo).
//!
//! - Cada equipo tiene su clave (`secret.key`, en su carpeta de datos): su
//!   identidad. Los vinculados se guardan en `peers.json`.
//! - Se anuncian en la red local por mDNS (servicio `atic-kvm`) con su nombre,
//!   así que para vincularse basta elegir el otro de una lista.
//! - Vincular ([`PAIR_ALPN`]): quien pide se presenta, el otro muestra un
//!   código de 6 dígitos y quien pide lo escribe. Un intento por conexión, y
//!   solo se atiende mientras el usuario abrió el vínculo en este equipo
//!   ([`Node::open_pairing`]), de a un pedido y con un tope de intentos fallidos.
//! - La sesión ([`ALPN`]) solo se acepta entre equipos vinculados.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use iroh::endpoint::{presets, Connection, ConnectionError, RecvStream, SendStream};
use iroh::endpoint_info::UserData;
use iroh::{Endpoint, EndpointAddr, EndpointId, SecretKey};
use iroh_mdns_address_lookup::{DiscoveryEvent, MdnsAddressLookup};
use n0_future::StreamExt;
use serde::{Deserialize, Serialize};

pub use crate::proto::{ALPN, PAIR_ALPN};
use crate::proto::{clean_name, clean_text, read_frame_max, write_frame, Msg, Os, PairMsg, MAX_GREETING, PAIR_BUSY, PAIR_CLOSED, PAIR_LOCKED};

const MDNS_SERVICE: &str = "atic-kvm";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const HELLO_TIMEOUT: Duration = Duration::from_secs(10);
/// Lo que tiene el usuario para escribir el código.
const CODE_TIMEOUT: Duration = Duration::from_secs(120);
/// Después de tantos códigos mostrados que no terminaron en vínculo, no se
/// atiende ninguno más por [`PAIR_LOCKOUT`].
const MAX_PAIR_FAILS: u32 = 5;
const PAIR_LOCKOUT: Duration = Duration::from_secs(10 * 60);
/// Lo más largo del motivo con que el otro rechaza (se le muestra al usuario).
const MAX_REASON: usize = 200;

/// Cuándo se atiende un pedido de vínculo: solo mientras el usuario lo abrió
/// en este equipo, de a uno, y no después de varios intentos fallidos. Sin
/// esto, cualquiera en la red podría probar códigos en paralelo sin parar.
#[derive(Default)]
struct PairGate {
    open_until: Option<Instant>,
    busy: bool,
    failures: u32,
    locked_until: Option<Instant>,
}

impl PairGate {
    /// Toma el turno, o dice con qué código se rechaza el pedido.
    fn enter(&mut self, now: Instant) -> Result<(), u32> {
        if self.locked_until.is_some_and(|until| now < until) {
            return Err(PAIR_LOCKED);
        }
        if !self.open_until.is_some_and(|until| now < until) {
            return Err(PAIR_CLOSED);
        }
        if self.busy {
            return Err(PAIR_BUSY);
        }
        self.busy = true;
        Ok(())
    }

    /// Se mostró un código y no hubo vínculo (equivocado, sin respuesta o cortado).
    fn failed(&mut self, now: Instant) {
        self.failures += 1;
        if self.failures >= MAX_PAIR_FAILS {
            self.failures = 0;
            self.locked_until = Some(now + PAIR_LOCKOUT);
            self.open_until = None;
        }
    }

    /// Vinculado: para otro equipo hay que abrirlo de nuevo.
    fn paired(&mut self) {
        self.failures = 0;
        self.open_until = None;
    }
}

/// El turno de un pedido de vínculo: al soltarlo queda libre para el siguiente.
struct PairTurn<'a>(&'a Mutex<PairGate>);

impl Drop for PairTurn<'_> {
    fn drop(&mut self) {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).busy = false;
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Peer {
    pub name: String,
    pub os: Os,
}

#[derive(Default, Serialize, Deserialize)]
struct PeerFile {
    peers: BTreeMap<String, Peer>,
}

pub struct Node {
    pub endpoint: Endpoint,
    mdns: MdnsAddressLookup,
    dir: PathBuf,
    pub name: String,
    peers: Mutex<BTreeMap<String, Peer>>,
    pairing: Mutex<PairGate>,
}

impl Node {
    /// Abre el endpoint con la identidad guardada en `dir` (o una nueva).
    pub async fn start(dir: &Path, name: String) -> Result<Arc<Self>> {
        std::fs::create_dir_all(dir).with_context(|| format!("no se pudo crear {}", dir.display()))?;
        let key = load_or_create_key(&dir.join("secret.key"))?;
        let endpoint = Endpoint::builder(presets::N0)
            .secret_key(key)
            .alpns(vec![ALPN.to_vec(), PAIR_ALPN.to_vec()])
            .bind()
            .await
            .map_err(|e| anyhow!("no se pudo abrir la conexión: {e:#}"))?;
        let mdns = MdnsAddressLookup::builder().service_name(MDNS_SERVICE).build(endpoint.id()).map_err(|e| anyhow!("sin mDNS: {e:#}"))?;
        endpoint.address_lookup().map_err(|e| anyhow!("{e:#}"))?.add(mdns.clone());
        let shown: String = name.chars().take(60).collect();
        if let Ok(data) = UserData::try_from(shown) {
            endpoint.set_user_data_for_address_lookup(Some(data));
        }
        let peers = std::fs::read(dir.join("peers.json"))
            .ok()
            .and_then(|b| serde_json::from_slice::<PeerFile>(&b).ok())
            .map(|f| f.peers)
            .unwrap_or_default();
        Ok(Arc::new(Self { endpoint, mdns, dir: dir.to_path_buf(), name, peers: Mutex::new(peers), pairing: Mutex::default() }))
    }

    fn gate(&self) -> std::sync::MutexGuard<'_, PairGate> {
        self.pairing.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Abre el vínculo por `window`: mientras tanto se atiende a un equipo que
    /// pida vincularse. Lo llama la app cuando el usuario está vinculando.
    pub fn open_pairing(&self, window: Duration) {
        self.gate().open_until = Some(Instant::now() + window);
    }

    pub fn close_pairing(&self) {
        self.gate().open_until = None;
    }

    pub fn pairing_open(&self) -> bool {
        self.gate().open_until.is_some_and(|until| Instant::now() < until)
    }

    pub fn id(&self) -> EndpointId {
        self.endpoint.id()
    }

    /// La carpeta de datos (identidad, vinculados y lo que llega para pegar).
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn peers(&self) -> Vec<(EndpointId, Peer)> {
        let peers = self.peers.lock().unwrap_or_else(|e| e.into_inner());
        peers.iter().filter_map(|(id, p)| Some((id.parse().ok()?, p.clone()))).collect()
    }

    pub fn peer(&self, id: &EndpointId) -> Option<Peer> {
        self.peers.lock().unwrap_or_else(|e| e.into_inner()).get(&id.to_string()).cloned()
    }

    fn add_peer(&self, id: EndpointId, peer: Peer) -> Result<()> {
        let mut peers = self.peers.lock().unwrap_or_else(|e| e.into_inner());
        peers.insert(id.to_string(), peer);
        let file = PeerFile { peers: peers.clone() };
        write_private(&self.dir.join("peers.json"), &serde_json::to_vec_pretty(&file)?)
    }

    /// Olvida un equipo: no se acepta más hasta que se vuelva a vincular.
    pub fn forget(&self, id: &EndpointId) -> Result<()> {
        let mut peers = self.peers.lock().unwrap_or_else(|e| e.into_inner());
        peers.remove(&id.to_string());
        let file = PeerFile { peers: peers.clone() };
        write_private(&self.dir.join("peers.json"), &serde_json::to_vec_pretty(&file)?)
    }

    /// Los equipos con esto abierto en la red local, con su nombre, después de escuchar `wait`.
    pub async fn nearby(&self, wait: Duration) -> Vec<(EndpointId, String)> {
        let mut events = self.mdns.subscribe().await;
        let mut found: BTreeMap<EndpointId, String> = BTreeMap::new();
        let deadline = tokio::time::Instant::now() + wait;
        while let Ok(Some(event)) = tokio::time::timeout_at(deadline, events.next()).await {
            match event {
                DiscoveryEvent::Discovered { endpoint_info, .. } => {
                    let name = clean_name(&endpoint_info.data.user_data().map(|d| d.to_string()).unwrap_or_default());
                    found.insert(endpoint_info.endpoint_id, name);
                }
                DiscoveryEvent::Expired { endpoint_id } => {
                    found.remove(&endpoint_id);
                }
                _ => {}
            }
        }
        found.remove(&self.id());
        found.into_iter().collect()
    }

    async fn dial(&self, id: EndpointId, alpn: &[u8]) -> Result<Connection> {
        tokio::time::timeout(CONNECT_TIMEOUT, self.endpoint.connect(EndpointAddr::new(id), alpn))
            .await
            .context("el otro equipo no contestó a tiempo")?
            .map_err(|e| anyhow!("no se pudo conectar: {e:#}"))
    }

    async fn open(&self, id: EndpointId, alpn: &[u8]) -> Result<(Connection, SendStream, RecvStream)> {
        let conn = self.dial(id, alpn).await?;
        let (send, recv) = conn.open_bi().await?;
        Ok((conn, send, recv))
    }

    /// Pide vincularse con `id`. `code` es el código que muestra el otro
    /// equipo: se espera recién cuando el otro avisa que ya lo mostró.
    pub async fn pair(&self, id: EndpointId, code: impl std::future::Future<Output = Result<String>>) -> Result<Peer> {
        let conn = self.dial(id, PAIR_ALPN).await?;
        match self.ask_pair(&conn, id, code).await {
            Ok(peer) => Ok(peer),
            // Si el otro cerró sin atender, lo que importa es por qué.
            Err(e) => match pair_refusal(&conn) {
                Some(reason) => bail!("{reason}"),
                None => Err(e),
            },
        }
    }

    async fn ask_pair(&self, conn: &Connection, id: EndpointId, code: impl std::future::Future<Output = Result<String>>) -> Result<Peer> {
        let (mut send, mut recv) = conn.open_bi().await?;
        write_frame(&mut send, &PairMsg::Ask { name: self.name.clone(), os: Os::current() }).await?;
        match tokio::time::timeout(HELLO_TIMEOUT, read_frame_max::<_, PairMsg>(&mut recv, MAX_GREETING)).await.context("el otro equipo no contestó")?? {
            Some(PairMsg::CodeShown) => {}
            Some(PairMsg::Refused { reason }) => bail!("{}", clean_text(&reason, MAX_REASON)),
            other => bail!("respuesta inesperada: {other:?}"),
        }
        let code = tokio::time::timeout(CODE_TIMEOUT, code).await.context("se acabó el tiempo para escribir el código")??;
        write_frame(&mut send, &PairMsg::Code { code: code.trim().to_string() }).await?;
        match read_frame_max::<_, PairMsg>(&mut recv, MAX_GREETING).await? {
            Some(PairMsg::Done { name, os }) => {
                let peer = Peer { name: clean_name(&name), os };
                self.add_peer(id, peer.clone())?;
                Ok(peer)
            }
            Some(PairMsg::Refused { reason }) => bail!("{}", clean_text(&reason, MAX_REASON)),
            other => bail!("respuesta inesperada: {other:?}"),
        }
    }

    /// Atiende un pedido de vínculo: `show_code` muestra el código en esta
    /// pantalla. `None` si no se atendió: el vínculo no está abierto
    /// ([`Node::open_pairing`]), ya se atiende otro pedido o hubo demasiados
    /// intentos fallidos. En ese caso se cierra la conexión sin leer nada.
    pub async fn serve_pair(&self, conn: Connection, show_code: impl Fn(&str, &str)) -> Result<Option<Peer>> {
        let entered = self.gate().enter(Instant::now());
        if let Err(code) = entered {
            conn.close(code.into(), b"");
            return Ok(None);
        }
        let _turn = PairTurn(&self.pairing);
        let id = conn.remote_id();
        let (mut send, mut recv) = tokio::time::timeout(HELLO_TIMEOUT, conn.accept_bi()).await.context("no mandó nada a tiempo")??;
        let Some(PairMsg::Ask { name, os }) = tokio::time::timeout(HELLO_TIMEOUT, read_frame_max(&mut recv, MAX_GREETING)).await.context("no se presentó")?? else {
            bail!("pedido de vínculo inválido");
        };
        let name = clean_name(&name);
        let code = new_code()?;
        show_code(&name, &code);
        let answered = async {
            write_frame(&mut send, &PairMsg::CodeShown).await?;
            let typed = match tokio::time::timeout(CODE_TIMEOUT, read_frame_max::<_, PairMsg>(&mut recv, MAX_GREETING)).await {
                Ok(Ok(Some(PairMsg::Code { code }))) => code,
                _ => bail!("{name} no escribió el código a tiempo"),
            };
            if !same(&typed, &code) {
                // Un intento por conexión, y una pausa antes de contestar.
                tokio::time::sleep(Duration::from_secs(2)).await;
                write_frame(&mut send, &PairMsg::Refused { reason: "El código no coincide.".into() }).await?;
                bail!("{name} escribió un código equivocado");
            }
            let peer = Peer { name: name.clone(), os };
            self.add_peer(id, peer.clone())?;
            Ok(peer)
        }
        .await;
        let peer = match answered {
            Ok(peer) => {
                self.gate().paired();
                peer
            }
            Err(e) => {
                self.gate().failed(Instant::now());
                return Err(e);
            }
        };
        write_frame(&mut send, &PairMsg::Done { name: self.name.clone(), os: Os::current() }).await?;
        // Que el otro alcance a leer la respuesta antes de cerrar.
        let _ = tokio::time::timeout(Duration::from_secs(3), conn.closed()).await;
        Ok(Some(peer))
    }

    /// Abre la sesión con un equipo vinculado: se saludan y queda lista.
    pub async fn connect(&self, id: EndpointId, hello: Msg) -> Result<Session> {
        let (conn, mut send, mut recv) = self.open(id, ALPN).await?;
        write_frame(&mut send, &hello).await?;
        let their = tokio::time::timeout(HELLO_TIMEOUT, read_frame_max::<_, Msg>(&mut recv, MAX_GREETING)).await.context("el otro equipo no saludó")??;
        let Some(their @ Msg::Hello { .. }) = their else { bail!("el otro equipo no saludó") };
        Ok(Session { conn, send, recv, hello: their })
    }

    /// Recibe una sesión ya aceptada por el endpoint: solo de equipos vinculados.
    pub async fn accept_session(&self, conn: Connection, hello: Msg) -> Result<Session> {
        if self.peer(&conn.remote_id()).is_none() {
            conn.close(1u32.into(), b"no vinculado");
            bail!("un equipo no vinculado intentó conectarse");
        }
        let (mut send, mut recv) = tokio::time::timeout(HELLO_TIMEOUT, conn.accept_bi()).await.context("no mandó nada a tiempo")??;
        let their = tokio::time::timeout(HELLO_TIMEOUT, read_frame_max::<_, Msg>(&mut recv, MAX_GREETING)).await.context("no saludó")??;
        let Some(their @ Msg::Hello { .. }) = their else { bail!("no saludó") };
        write_frame(&mut send, &hello).await?;
        Ok(Session { conn, send, recv, hello: their })
    }
}

pub struct Session {
    pub conn: Connection,
    pub send: SendStream,
    pub recv: RecvStream,
    /// El saludo del otro ([`Msg::Hello`]).
    pub hello: Msg,
}

fn load_or_create_key(path: &Path) -> Result<SecretKey> {
    if let Ok(bytes) = std::fs::read(path) {
        let bytes: [u8; 32] = bytes.as_slice().try_into().map_err(|_| anyhow!("{} está dañada", path.display()))?;
        return Ok(SecretKey::from_bytes(&bytes));
    }
    let key = SecretKey::generate();
    write_private(path, &key.to_bytes())?;
    Ok(key)
}

/// Escribe un archivo que solo lee este usuario (en Windows ya lo es la carpeta de datos).
fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    let tmp = path.with_extension("tmp");
    // Uno que haya quedado de antes conservaría sus permisos.
    let _ = std::fs::remove_file(&tmp);
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    // Privado desde que se crea: la clave no pasa ni un momento legible por otros.
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options.open(&tmp)?.write_all(bytes)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Por qué el otro cerró el pedido de vínculo sin atenderlo, si fue eso.
fn pair_refusal(conn: &Connection) -> Option<&'static str> {
    let Some(ConnectionError::ApplicationClosed(close)) = conn.close_reason() else { return None };
    let reasons = [
        (PAIR_CLOSED, "El otro computador no está esperando un vínculo. Busca computadores también en él y vuelve a intentar."),
        (PAIR_BUSY, "El otro computador está atendiendo otro pedido de vínculo. Intenta de nuevo en un momento."),
        (PAIR_LOCKED, "El otro computador dejó de aceptar vínculos por varios intentos fallidos. Intenta de nuevo en unos minutos."),
    ];
    reasons.into_iter().find(|(code, _)| close.error_code == (*code).into()).map(|(_, reason)| reason)
}

fn new_code() -> Result<String> {
    let mut b = [0u8; 4];
    getrandom::fill(&mut b).map_err(|e| anyhow!("{e}"))?;
    Ok(format!("{:06}", u32::from_le_bytes(b) % 1_000_000))
}

/// Compara sin cortar en la primera diferencia.
fn same(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proto::read_frame;

    #[test]
    fn el_codigo_tiene_seis_digitos() {
        for _ in 0..100 {
            let c = new_code().unwrap();
            assert_eq!(c.len(), 6);
            assert!(c.bytes().all(|b| b.is_ascii_digit()));
        }
        assert!(same("012345", "012345"));
        assert!(!same("012345", "012346"));
        assert!(!same("01234", "012345"));
    }

    /// Dos equipos en el mismo proceso: se vinculan con el código y abren una sesión.
    #[tokio::test]
    async fn se_vinculan_y_abren_la_sesion() {
        let base = std::env::temp_dir().join(format!("atic-kvm-test-{}", std::process::id()));
        let a = Node::start(&base.join("a"), "PC".into()).await.unwrap();
        let b = Node::start(&base.join("b"), "Mac".into()).await.unwrap();
        // El endpoint de `b` en el de `a`, sin depender del mDNS de la red.
        let b_addr = b.endpoint.addr();
        a.endpoint.address_lookup().unwrap().add(iroh::address_lookup::MemoryLookup::from_endpoint_info([b_addr]));
        b.open_pairing(Duration::from_secs(60));

        let (code_tx, code_rx) = std::sync::mpsc::channel::<String>();
        let server = {
            let b = b.clone();
            tokio::spawn(async move {
                let conn = b.endpoint.accept().await.unwrap().accept().unwrap().await.unwrap();
                assert_eq!(conn.alpn(), PAIR_ALPN);
                assert!(b.serve_pair(conn, move |_, code| code_tx.send(code.to_string()).unwrap()).await.unwrap().is_some());
                // Vinculado: para otro equipo hay que abrirlo de nuevo.
                assert!(!b.pairing_open());
                let conn = b.endpoint.accept().await.unwrap().accept().unwrap().await.unwrap();
                let hello = Msg::Hello { name: "Mac".into(), os: Os::Macos, screens: vec![], layout: None };
                let mut s = b.accept_session(conn, hello).await.unwrap();
                let got: Msg = read_frame(&mut s.recv).await.unwrap().unwrap();
                assert_eq!(got, Msg::Move { dx: 1, dy: 2 });
                write_frame(&mut s.send, &Msg::Back { along: 0.5 }).await.unwrap();
                let _ = tokio::time::timeout(Duration::from_secs(3), s.conn.closed()).await;
            })
        };
        let code = async move { Ok(tokio::task::spawn_blocking(move || code_rx.recv()).await??) };
        let peer = a.pair(b.id(), code).await.unwrap();
        assert_eq!(peer.name, "Mac");
        assert!(b.peer(&a.id()).is_some());

        let hello = Msg::Hello { name: "PC".into(), os: Os::Windows, screens: vec![], layout: None };
        let mut s = a.connect(b.id(), hello).await.unwrap();
        assert!(matches!(s.hello, Msg::Hello { os: Os::Macos, .. }));
        write_frame(&mut s.send, &Msg::Move { dx: 1, dy: 2 }).await.unwrap();
        let back: Msg = read_frame(&mut s.recv).await.unwrap().unwrap();
        assert_eq!(back, Msg::Back { along: 0.5 });
        s.conn.close(0u32.into(), b"");
        server.await.unwrap();
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn el_vinculo_solo_se_atiende_abierto_de_a_uno_y_con_tope_de_fallos() {
        let now = Instant::now();
        let mut gate = PairGate::default();
        // Nadie lo abrió.
        assert_eq!(gate.enter(now), Err(PAIR_CLOSED));
        gate.open_until = Some(now + Duration::from_secs(60));
        assert_eq!(gate.enter(now), Ok(()));
        // De a uno: el segundo se rechaza mientras el primero no termine.
        assert_eq!(gate.enter(now), Err(PAIR_BUSY));
        gate.busy = false;
        // Pasado el plazo, cerrado otra vez.
        assert_eq!(gate.enter(now + Duration::from_secs(61)), Err(PAIR_CLOSED));

        // Al quinto fallo no se atiende a nadie por un rato, aunque se abra de nuevo.
        for _ in 0..MAX_PAIR_FAILS - 1 {
            gate.failed(now);
        }
        assert_eq!(gate.enter(now), Ok(()));
        gate.busy = false;
        gate.failed(now);
        gate.open_until = Some(now + PAIR_LOCKOUT * 2);
        assert_eq!(gate.enter(now), Err(PAIR_LOCKED));
        assert_eq!(gate.enter(now + PAIR_LOCKOUT - Duration::from_secs(1)), Err(PAIR_LOCKED));
        assert_eq!(gate.enter(now + PAIR_LOCKOUT), Ok(()));
        gate.busy = false;

        // Un vínculo logrado lo cierra y borra los fallos.
        gate.failed(now);
        gate.paired();
        assert_eq!(gate.failures, 0);
        assert_eq!(gate.enter(now + PAIR_LOCKOUT), Err(PAIR_CLOSED));
    }

    /// Un equipo que nadie abrió para vincular cierra el pedido sin mostrar
    /// código; abierto, atiende uno solo a la vez.
    #[tokio::test]
    async fn sin_abrir_el_vinculo_no_se_muestra_codigo() {
        let base = std::env::temp_dir().join(format!("atic-kvm-gate-{}", std::process::id()));
        let a = Node::start(&base.join("a"), "PC".into()).await.unwrap();
        let b = Node::start(&base.join("b"), "Mac".into()).await.unwrap();
        let c = Node::start(&base.join("c"), "Otro".into()).await.unwrap();
        for node in [&a, &c] {
            node.endpoint.address_lookup().unwrap().add(iroh::address_lookup::MemoryLookup::from_endpoint_info([b.endpoint.addr()]));
        }
        let accept = |b: Arc<Node>| async move { b.endpoint.accept().await.unwrap().accept().unwrap().await.unwrap() };
        let never = || async { anyhow::Ok(std::future::pending::<String>().await) };

        // Cerrado: ni código ni vínculo, y quien pide sabe por qué.
        let server = {
            let b = b.clone();
            tokio::spawn(async move {
                let conn = accept(b.clone()).await;
                b.serve_pair(conn, |_, _| panic!("no debería mostrar un código")).await.unwrap()
            })
        };
        let refused = a.pair(b.id(), never()).await.unwrap_err();
        assert!(refused.to_string().contains("no está esperando"), "{refused:#}");
        assert_eq!(server.await.unwrap(), None);
        assert!(b.peers().is_empty());

        // Abierto: el primero recibe su código; el segundo, mientras tanto, no.
        b.open_pairing(Duration::from_secs(60));
        let (shown_tx, shown_rx) = tokio::sync::oneshot::channel::<()>();
        let shown_tx = Mutex::new(Some(shown_tx));
        let first = {
            let b = b.clone();
            tokio::spawn(async move {
                let conn = accept(b.clone()).await;
                let show = move |_: &str, _: &str| {
                    let _ = shown_tx.lock().unwrap().take().unwrap().send(());
                };
                b.serve_pair(conn, show).await
            })
        };
        let asking = {
            let a = a.clone();
            let id = b.id();
            tokio::spawn(async move { a.pair(id, never()).await })
        };
        shown_rx.await.unwrap();
        let second = {
            let b = b.clone();
            tokio::spawn(async move {
                let conn = accept(b.clone()).await;
                b.serve_pair(conn, |_, _| panic!("no debería mostrar un segundo código")).await.unwrap()
            })
        };
        let busy = c.pair(b.id(), never()).await.unwrap_err();
        assert!(busy.to_string().contains("otro pedido"), "{busy:#}");
        assert_eq!(second.await.unwrap(), None);

        // El primero se va sin escribir el código: cuenta como intento fallido.
        asking.abort();
        let _ = asking.await;
        a.endpoint.close().await;
        assert!(first.await.unwrap().is_err());
        assert_eq!(b.gate().failures, 1);
        assert!(!b.gate().busy);
        let _ = std::fs::remove_dir_all(base);
    }
}
