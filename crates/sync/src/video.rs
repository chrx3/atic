//! Video entre el celular y el PC: la pantalla del celular en el PC (y, más adelante, la
//! del PC en el celular y la cámara).
//!
//! Va por una conexión QUIC aparte, con su propio ALPN ([`VIDEO_ALPN`]), que abre el
//! celular con el mismo endpoint de iroh: un cuadro grande no frena los mensajes ni los
//! archivos de `atic/sync/1`, cada conexión regula su congestión y las versiones viejas
//! no se enteran.
//!
//! 1. El celular abre un bi-stream y manda [`VideoHello`] (un frame JSON, como los
//!    mensajes). El PC contesta [`VideoReply`]: solo acepta un equipo pareado con la
//!    función [`crate::feature::SCREEN`] prendida y si quien muestra el video quiere.
//! 2. En el sentido del video van cuadros binarios ([`VideoFrame`]):
//!    `[u32 BE largo][u8 flags][u64 BE pts_us][datos]`. Los datos son H.264 en Annex-B
//!    ([`flags::CONFIG`]: SPS/PPS; [`flags::KEY`]: keyframe), o un [`VideoFormat`] en
//!    JSON ([`flags::FORMAT`]) cuando cambian el tamaño o la rotación (el codificador se
//!    reinicia con otro SPS/PPS). `pts_us` cuenta desde el inicio de la sesión.
//! 3. En el sentido contrario van frames JSON de control ([`VideoControl`]).
//!
//! Quien manda no acumula: si se atrasa, descarta hasta el próximo keyframe (y lo pide).
//! Quien recibe pide un keyframe al empezar y después de un hueco.
//!
//! **`atic/video/2`** ([`VIDEO_ALPN_2`]) cambia solo el paso 2: cada cuadro va en su
//! propio stream unidireccional (`[u8 flags][u64 BE pts_us][datos hasta el final]`),
//! abiertos en orden y con prioridad para los más viejos. En una sola corriente, lo que
//! ya está en el buffer de QUIC sale aunque llegue tarde; así, quien manda abandona
//! (`reset`) los cuadros que llevan demasiado en camino, pide un keyframe y baja el
//! bitrate. Quien manda mide cuánto tarda cada cuadro en confirmarse y ajusta el
//! bitrate solo ([`VideoControl::SetBitrate`] a su codificador). Al terminar, quien
//! manda cierra su lado del saludo. El celular ofrece las dos versiones y el PC elige.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use iroh::endpoint::{ConnectOptions, Connection, ConnectionError, QuicTransportConfig, ReadError, ReadToEndError, RecvStream, SendStream};
use iroh::{Endpoint, EndpointAddr};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use noq_proto::congestion::Bbr3Config;
use tokio::sync::{mpsc, oneshot};

use crate::proto::{read_frame_max, write_frame};

/// El ALPN del video: un cambio incompatible cambia el número.
pub const VIDEO_ALPN: &[u8] = b"atic/video/1";
/// Un cuadro por stream, con bitrate adaptable (ver arriba).
pub const VIDEO_ALPN_2: &[u8] = b"atic/video/2";

/// `true` si la conexión es de video (cualquier versión).
pub fn is_video_alpn(alpn: &[u8]) -> bool {
    alpn == VIDEO_ALPN || alpn == VIDEO_ALPN_2
}

/// Tope de un cuadro (un keyframe 4K en H.264 cabe de sobra).
pub const MAX_VIDEO_FRAME: usize = 4 << 20;
/// Tope de un mensaje JSON del video (saludo, formato, control).
const MAX_VIDEO_MESSAGE: u32 = 16 * 1024;
/// Cuadros en espera a cada lado: más, y el video se atrasa en vez de saltar.
const QUEUE: usize = 4;
/// v2: la cabecera de un cuadro en su stream (flags y pts).
const HEAD_2: usize = 9;
/// v2: cuadros en camino como mucho; con más, quien manda espera (y su cola se llena).
const MAX_IN_FLIGHT: usize = 12;
/// v2: un cuadro común que lleva esto sin confirmarse ya no sirve: se abandona.
const STALE: Duration = Duration::from_millis(500);
/// v2: el bitrate más bajo al que se baja.
const MIN_BITRATE: u32 = 300_000;
/// v2: el bitrate de partida si el camino va por relay.
const RELAY_BITRATE: u32 = 1_500_000;
/// Cada cuánto se escriben las estadísticas en el log.
const STATS_EVERY: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VideoSource {
    Screen,
    Camera,
}

/// Desde el lado del celular: `Send` manda su video; `Receive` pide el del PC.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VideoDirection {
    Send,
    Receive,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VideoHello {
    pub source: VideoSource,
    pub direction: VideoDirection,
    /// `h264` por ahora.
    pub codec: String,
    pub width: u32,
    pub height: u32,
    /// Grados (0, 90, 180, 270) en que hay que girar la imagen para verla derecha.
    pub rotation: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VideoReply {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Tamaño y rotación nuevos a mitad de la sesión (el celular giró).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct VideoFormat {
    pub width: u32,
    pub height: u32,
    pub rotation: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum VideoControl {
    RequestKeyframe,
    SetBitrate { bps: u32 },
    Stop,
}

pub mod flags {
    pub const KEY: u8 = 1;
    pub const CONFIG: u8 = 1 << 1;
    pub const FORMAT: u8 = 1 << 2;
}

#[derive(Clone, Debug, PartialEq)]
pub struct VideoFrame {
    pub flags: u8,
    pub pts_us: u64,
    pub data: Vec<u8>,
}

impl VideoFrame {
    pub fn is_key(&self) -> bool {
        self.flags & (flags::KEY | flags::CONFIG | flags::FORMAT) != 0
    }

    /// El formato nuevo, si este cuadro lo trae.
    pub fn format(&self) -> Option<VideoFormat> {
        (self.flags & flags::FORMAT != 0).then(|| serde_json::from_slice(&self.data).ok()).flatten()
    }
}

pub async fn write_video_frame(w: &mut (impl AsyncWriteExt + Unpin), frame: &VideoFrame) -> Result<()> {
    if frame.data.len() > MAX_VIDEO_FRAME {
        bail!("cuadro demasiado grande ({} bytes)", frame.data.len());
    }
    let mut head = [0u8; 13];
    head[..4].copy_from_slice(&(frame.data.len() as u32).to_be_bytes());
    head[4] = frame.flags;
    head[5..].copy_from_slice(&frame.pts_us.to_be_bytes());
    w.write_all(&head).await?;
    w.write_all(&frame.data).await?;
    Ok(())
}

/// `None` si la corriente terminó limpio.
pub async fn read_video_frame(r: &mut (impl AsyncReadExt + Unpin)) -> Result<Option<VideoFrame>> {
    let mut head = [0u8; 13];
    match r.read_exact(&mut head[..1]).await {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e.into()),
    }
    r.read_exact(&mut head[1..]).await?;
    let len = u32::from_be_bytes(head[..4].try_into().unwrap()) as usize;
    if len > MAX_VIDEO_FRAME {
        bail!("cuadro demasiado grande ({len} bytes)");
    }
    let mut data = vec![0u8; len];
    r.read_exact(&mut data).await?;
    Ok(Some(VideoFrame { flags: head[4], pts_us: u64::from_be_bytes(head[5..].try_into().unwrap()), data }))
}

// --- Transporte: v1 (una corriente) o v2 (un stream por cuadro) ------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Wire {
    V1,
    V2,
}

impl Wire {
    fn of(conn: &Connection) -> Self {
        if conn.alpn() == VIDEO_ALPN_2 {
            Wire::V2
        } else {
            Wire::V1
        }
    }
}

/// El celular abre la conexión de video ofreciendo v2 y v1 (un PC viejo elige v1), con
/// BBR en vez de Cubic para lo que manda: con pérdida (Wi-Fi malo, datos móviles) Cubic
/// se frena solo y BBR no (en la red mala simulada, con 2 % de pérdida, ~2 Mb/s contra
/// ~3,5). El control de congestión es de cada lado: lo que manda el PC sigue con el suyo.
async fn connect(endpoint: &Endpoint, addr: EndpointAddr) -> Result<Connection> {
    let transport = QuicTransportConfig::builder().congestion_controller_factory(Arc::new(Bbr3Config::default())).build();
    let options = ConnectOptions::new().with_additional_alpns(vec![VIDEO_ALPN.to_vec()]).with_transport_config(transport);
    let connecting = endpoint.connect_with_opts(addr, VIDEO_ALPN_2, options).await.map_err(|e| anyhow::anyhow!("No se pudo llegar al PC: {e:#}"))?;
    connecting.await.map_err(|e| anyhow::anyhow!("No se pudo llegar al PC: {e:#}"))
}

/// El camino que usa la conexión ahora y su RTT.
fn path_of(conn: &Connection) -> (&'static str, Option<Duration>) {
    let paths = conn.paths();
    match paths.iter().find(|p| p.is_selected()) {
        Some(path) => (if path.is_relay() { "relay" } else { "directo" }, Some(path.rtt())),
        None => ("sin camino", None),
    }
}

/// Lo que llega de quien manda el video.
#[derive(Debug)]
enum Incoming {
    Frame(VideoFrame),
    /// Quien manda abandonó un cuadro atrasado (v2): sirve recién el próximo keyframe.
    Dropped,
}

/// Lee los cuadros en su propia tarea: cortar una lectura a medias (otra rama de un
/// `select!` que gana) dejaría la corriente desfasada. La cola se cierra cuando quien
/// manda termina; un error llega como último mensaje.
fn spawn_reader(wire: Wire, conn: Connection, recv: RecvStream, label: &'static str) -> mpsc::Receiver<Result<Incoming>> {
    let (tx, rx) = mpsc::channel(QUEUE * 2);
    tokio::spawn(async move {
        let result = match wire {
            Wire::V1 => read_v1(recv, &tx).await,
            Wire::V2 => read_v2(&conn, recv, &tx, label).await,
        };
        if let Err(e) = result {
            let _ = tx.send(Err(e)).await;
        }
    });
    rx
}

async fn read_v1(mut recv: RecvStream, tx: &mpsc::Sender<Result<Incoming>>) -> Result<()> {
    while let Some(frame) = read_video_frame(&mut recv).await? {
        if tx.send(Ok(Incoming::Frame(frame))).await.is_err() {
            break;
        }
    }
    Ok(())
}

/// `true` si la conexión terminó porque alguno de los dos la cerró (no es un error).
fn closed_on_purpose(e: &ConnectionError) -> bool {
    matches!(e, ConnectionError::ApplicationClosed(_) | ConnectionError::LocallyClosed)
}

async fn read_v2(conn: &Connection, mut ctrl: RecvStream, tx: &mpsc::Sender<Result<Incoming>>, label: &str) -> Result<()> {
    let mut stats = Stats::new();
    loop {
        let mut stream = tokio::select! {
            stream = conn.accept_uni() => match stream {
                Ok(stream) => stream,
                Err(e) if closed_on_purpose(&e) => return Ok(()),
                Err(e) => return Err(e.into()),
            },
            // Quien manda cerró su lado del saludo: terminó.
            () = until_end(&mut ctrl) => return Ok(()),
        };
        let incoming = match stream.read_to_end(HEAD_2 + MAX_VIDEO_FRAME).await {
            Ok(mut bytes) => {
                if bytes.len() < HEAD_2 {
                    bail!("cuadro sin cabecera ({} bytes)", bytes.len());
                }
                let flags = bytes[0];
                let pts_us = u64::from_be_bytes(bytes[1..HEAD_2].try_into().unwrap());
                bytes.drain(..HEAD_2);
                stats.frames += 1;
                stats.bytes += bytes.len();
                Incoming::Frame(VideoFrame { flags, pts_us, data: bytes })
            }
            Err(ReadToEndError::Read(ReadError::Reset(_))) => {
                stats.dropped += 1;
                Incoming::Dropped
            }
            Err(ReadToEndError::Read(ReadError::ConnectionLost(e))) if closed_on_purpose(&e) => return Ok(()),
            Err(e) => return Err(e.into()),
        };
        if tx.send(Ok(incoming)).await.is_err() {
            return Ok(());
        }
        if let Some(line) = stats.report() {
            tracing::info!("{label} (recibe): {line}, {}", path_of(conn).0);
        }
    }
}

/// Espera a que se cierre la corriente (lo que llegue se ignora). Se puede cortar.
async fn until_end(recv: &mut RecvStream) {
    let mut buf = [0u8; 64];
    while let Ok(Some(_)) = recv.read(&mut buf).await {}
}

/// Lo que se cuenta para el log: cuadros, bytes y descartados desde el último informe.
struct Stats {
    since: Instant,
    frames: usize,
    bytes: usize,
    dropped: usize,
}

impl Stats {
    fn new() -> Self {
        Self { since: Instant::now(), frames: 0, bytes: 0, dropped: 0 }
    }

    /// Cada [`STATS_EVERY`], una línea con lo de ese rato (y se empieza de nuevo).
    fn report(&mut self) -> Option<String> {
        let secs = self.since.elapsed().as_secs_f32();
        if secs < STATS_EVERY.as_secs_f32() {
            return None;
        }
        let line = format!(
            "{:.0} cuadros/s, {:.0} kb/s, {} descartados",
            self.frames as f32 / secs,
            self.bytes as f32 * 8. / 1000. / secs,
            self.dropped,
        );
        *self = Self::new();
        Some(line)
    }
}

/// Manda los cuadros de `frames` hasta que se cierra (Ok) o se corta la conexión.
/// v1 los escribe en orden en `send`; v2 usa un stream por cuadro y adapta el bitrate:
/// lo que decide le llega a `on_control` (como si lo pidiera quien mira).
async fn pump(
    wire: Wire,
    conn: &Connection,
    send: &mut SendStream,
    frames: &mut mpsc::Receiver<VideoFrame>,
    on_control: &(dyn Fn(VideoControl) + Send + Sync),
    max_bitrate: u32,
    label: &str,
) -> Result<()> {
    if wire == Wire::V1 {
        while let Some(frame) = frames.recv().await {
            write_video_frame(send, &frame).await?;
        }
        return Ok(());
    }
    let (path, _) = path_of(conn);
    let mut rate = RateControl::new(max_bitrate, if path == "relay" { RELAY_BITRATE } else { max_bitrate });
    if rate.target != max_bitrate {
        on_control(VideoControl::SetBitrate { bps: rate.target });
    }
    let (done_tx, mut done) = mpsc::unbounded_channel::<(u64, bool)>();
    let mut flights: VecDeque<Flight> = VecDeque::new();
    let mut need_key = false;
    let mut seq = 0u64;
    let mut stats = Stats::new();
    let mut tick = tokio::time::interval(Duration::from_millis(100));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            frame = frames.recv(), if flights.len() < MAX_IN_FLIGHT => {
                let Some(frame) = frame else { return Ok(()) };
                if need_key && !frame.is_key() {
                    stats.dropped += 1;
                    continue;
                }
                need_key = false;
                let stream = conn.open_uni().await?;
                // Primero los más viejos: un cuadro sirve solo si llegaron los anteriores.
                stream.set_priority(i32::MAX - seq.min(i32::MAX as u64) as i32).ok();
                let (cancel, cancelled) = oneshot::channel();
                flights.push_back(Flight { seq, flags: frame.flags, bytes: frame.data.len(), sent: Instant::now(), cancel: Some(cancel) });
                stats.frames += 1;
                stats.bytes += frame.data.len();
                let done_tx = done_tx.clone();
                let id = seq;
                tokio::spawn(async move {
                    let delivered = send_one(stream, frame, cancelled).await;
                    let _ = done_tx.send((id, delivered));
                });
                seq += 1;
            }
            Some((id, delivered)) = done.recv() => {
                if let Some(i) = flights.iter().position(|f| f.seq == id) {
                    let flight = flights.remove(i).expect("está");
                    if delivered {
                        rate.on_delivered(&flight, Instant::now());
                    }
                }
            }
            _ = tick.tick() => {
                let now = Instant::now();
                // Cuadros comunes que llevan demasiado en camino: ya no sirven. Se abandonan
                // todos los comunes (los que siguen dependen de ellos) y se pide un keyframe.
                // Con un keyframe en camino no: pedir otro sería otro golpe igual de grande
                // (la cola la baja el bitrate).
                let key_in_flight = flights.iter().any(|f| f.flags & flags::KEY != 0);
                if !key_in_flight && flights.iter().any(|f| f.flags == 0 && f.cancel.is_some() && now - f.sent > STALE) {
                    for flight in flights.iter_mut().filter(|f| f.flags == 0) {
                        if let Some(cancel) = flight.cancel.take() {
                            let _ = cancel.send(());
                            stats.dropped += 1;
                        }
                    }
                    need_key = true;
                    on_control(VideoControl::RequestKeyframe);
                    rate.on_stale(now);
                    on_control(VideoControl::SetBitrate { bps: rate.target });
                }
                let oldest = flights.iter().find(|f| rate.counts(f)).map(|f| now - f.sent).unwrap_or_default();
                if let Some(bps) = rate.update(now, oldest) {
                    on_control(VideoControl::SetBitrate { bps });
                }
                if let Some(line) = stats.report() {
                    let (path, rtt) = path_of(conn);
                    tracing::info!(
                        "{label} (manda): {line}, objetivo {} kb/s, cola {} ms, rtt {} ms, {path}",
                        rate.target / 1000,
                        rate.last_queue.as_millis(),
                        rtt.map(|r| r.as_millis() as i64).unwrap_or(-1),
                    );
                }
            }
        }
    }
}

/// `bps × factor`, redondeado al kb/s.
fn scaled(bps: u32, factor: f64) -> u32 {
    ((bps as f64 * factor / 1000.).round() as u32) * 1000
}

/// Un cuadro en camino (v2).
struct Flight {
    seq: u64,
    flags: u8,
    bytes: usize,
    sent: Instant,
    /// Para abandonarlo; `None` si ya se abandonó.
    cancel: Option<oneshot::Sender<()>>,
}

/// Escribe un cuadro en su stream y espera la confirmación de que llegó entero. `false`
/// si se abandonó (o se cortó) antes.
async fn send_one(mut stream: SendStream, frame: VideoFrame, cancelled: oneshot::Receiver<()>) -> bool {
    let mut head = [0u8; HEAD_2];
    head[0] = frame.flags;
    head[1..].copy_from_slice(&frame.pts_us.to_be_bytes());
    let delivered = tokio::select! {
        sent = async {
            stream.write_all(&head).await?;
            stream.write_all(&frame.data).await?;
            stream.finish()?;
            anyhow::Ok(stream.stopped().await?.is_none())
        } => sent.unwrap_or(false),
        // Abandonado (o quien manda terminó y soltó el aviso).
        _ = cancelled => false,
    };
    if !delivered {
        stream.reset(0u32.into()).ok();
    }
    delivered
}

/// El bitrate que se pide al codificador según cuánto tardan los cuadros en confirmarse.
///
/// La demora más baja que se vio hace poco es el viaje sin cola (RTT más la demora de los
/// acks); lo que pase de ahí es cola en la red. Con cola se baja ×0,7 (como mucho cada
/// 0,8 s); sin cola y usando al menos la mitad del bitrate se sube ×1,15 (cada 1 s).
/// Un cuadro abandonado lo baja a la mitad. Los keyframes no cuentan: tardan por grandes;
/// tampoco los que salieron antes de la última bajada: su demora es la cola de antes.
struct RateControl {
    max: u32,
    target: u32,
    /// La demora más baja de este rato y la del anterior (se renueva cada 5 s): si el
    /// camino cambia (de directo a relay), en 10 s como mucho es la del camino nuevo.
    base: (Option<Duration>, Option<Duration>),
    base_since: Instant,
    queue: Vec<Duration>,
    bytes: usize,
    window_since: Instant,
    last_up: Instant,
    last_down: Instant,
    /// La cola promedio del último rato, para el log.
    last_queue: Duration,
}

impl RateControl {
    fn new(max: u32, start: u32) -> Self {
        let now = Instant::now();
        Self {
            max: max.max(MIN_BITRATE),
            target: start.clamp(MIN_BITRATE, max.max(MIN_BITRATE)),
            base: (None, None),
            base_since: now,
            queue: Vec::new(),
            bytes: 0,
            window_since: now,
            last_up: now,
            last_down: now.checked_sub(Duration::from_secs(10)).unwrap_or(now),
            last_queue: Duration::ZERO,
        }
    }

    fn base(&self) -> Option<Duration> {
        match self.base {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    /// `false` si el cuadro salió antes de la última bajada.
    fn counts(&self, flight: &Flight) -> bool {
        flight.sent >= self.last_down
    }

    fn on_delivered(&mut self, flight: &Flight, now: Instant) {
        let delay = now - flight.sent;
        if now - self.base_since > Duration::from_secs(5) {
            self.base = (None, self.base.0);
            self.base_since = now;
        }
        self.base.0 = Some(self.base.0.map_or(delay, |b| b.min(delay)));
        self.bytes += flight.bytes;
        if flight.flags == 0 && self.counts(flight) {
            if let Some(base) = self.base() {
                self.queue.push(delay.saturating_sub(base));
            }
        }
    }

    /// También olvida la demora de base: tras un corte así, lo más probable es que haya
    /// cambiado la red (otro camino, otro RTT).
    fn on_stale(&mut self, now: Instant) {
        self.target = (self.target / 2).max(MIN_BITRATE);
        self.last_down = now;
        self.base = (None, None);
        self.base_since = now;
        self.queue.clear();
    }

    /// Cada medio segundo decide; `Some(bps)` si cambió.
    fn update(&mut self, now: Instant, oldest_in_flight: Duration) -> Option<u32> {
        let window = now - self.window_since;
        if window < Duration::from_millis(500) {
            return None;
        }
        let queue = if self.queue.is_empty() { Duration::ZERO } else { self.queue.iter().sum::<Duration>() / self.queue.len() as u32 };
        let used = self.bytes as f64 * 8. / window.as_secs_f64();
        let samples = self.queue.len();
        self.last_queue = queue;
        self.queue.clear();
        self.bytes = 0;
        self.window_since = now;
        let before = self.target;
        let congested = queue > Duration::from_millis(150) || oldest_in_flight > Duration::from_millis(350);
        if congested {
            if now - self.last_down > Duration::from_millis(800) {
                self.target = scaled(self.target, 0.7).max(MIN_BITRATE);
                self.last_down = now;
            }
        } else if queue < Duration::from_millis(40)
            && samples >= 5
            && used >= self.target as f64 * 0.5
            && now - self.last_up > Duration::from_secs(1)
            && now - self.last_down > Duration::from_secs(2)
        {
            self.target = scaled(self.target, 1.15).min(self.max);
            self.last_up = now;
        }
        (self.target != before).then_some(self.target)
    }
}

// --- Lado del PC ---------------------------------------------------------------------

/// Un video que llega al PC: los cuadros y el canal para pedirle cosas al celular.
pub struct VideoLink {
    pub device_id: String,
    pub hello: VideoHello,
    pub frames: mpsc::Receiver<VideoFrame>,
    pub control: mpsc::UnboundedSender<VideoControl>,
}

/// Quien muestra el video en el PC: recibe cada pedido y decide (`Err(motivo)` lo rechaza).
pub type VideoHandler = dyn Fn(VideoLink) -> std::result::Result<(), String> + Send + Sync;

/// La pantalla del PC que pide ver un celular: su id y el ancho máximo que quiere (el
/// PC escala manteniendo la proporción).
#[derive(Clone, Debug, PartialEq)]
pub struct ScreenRequest {
    pub device_id: String,
    pub max_width: u32,
}

/// Lo que entrega quien comparte la pantalla del PC: los cuadros (H.264 Annex-B, con
/// FORMAT al empezar y si cambia el tamaño) y por dónde le llegan los pedidos del
/// celular. Cuando el celular deja de mirar, llega `Stop` y `frames` se cierra.
pub struct ScreenSource {
    pub frames: mpsc::Receiver<VideoFrame>,
    pub control: mpsc::UnboundedSender<VideoControl>,
    /// El bitrate con que parte el codificador: v2 no pide más que esto.
    pub max_bitrate: u32,
}

/// Quien comparte la pantalla del PC: recibe cada pedido y decide (`Err(motivo)` lo rechaza).
pub type ScreenHandler = dyn Fn(ScreenRequest) -> std::result::Result<ScreenSource, String> + Send + Sync;

/// Lo que puede hacer el PC con una conexión de video: quién muestra el video que llega
/// y quién comparte su pantalla, si el equipo y las funciones lo permiten (lo sabe
/// `desktop.rs`; `Err` lleva el motivo).
pub(crate) struct DesktopVideo {
    pub show: std::result::Result<Arc<VideoHandler>, String>,
    pub share: std::result::Result<Arc<ScreenHandler>, String>,
}

/// Atiende una conexión `atic/video/1` en el PC.
pub(crate) async fn serve_desktop(conn: Connection, video: DesktopVideo) -> Result<()> {
    let device_id = conn.remote_id().to_string();
    let (mut send, mut recv) = conn.accept_bi().await?;
    let hello: VideoHello = read_frame_max(&mut recv, MAX_VIDEO_MESSAGE).await?.context("cerró sin saludar")?;
    let refuse = |reason: String| VideoReply { ok: false, reason: Some(reason) };
    if hello.codec != "h264" {
        write_frame(&mut send, &refuse(format!("Formato de video no soportado: {}", hello.codec))).await?;
        send.finish()?;
        send.stopped().await.ok();
        return Ok(());
    }
    let wire = Wire::of(&conn);
    if hello.direction == VideoDirection::Receive {
        return share_screen(&conn, wire, send, recv, ScreenRequest { device_id, max_width: hello.width }, video.share).await;
    }
    let decided = video.show;
    let handler = match decided {
        Ok(handler) => handler,
        Err(reason) => {
            write_frame(&mut send, &refuse(reason)).await?;
            send.finish()?;
            send.stopped().await.ok();
            return Ok(());
        }
    };
    let (frames_tx, frames) = mpsc::channel(QUEUE);
    let (control, mut control_rx) = mpsc::unbounded_channel();
    if let Err(reason) = handler(VideoLink { device_id: device_id.clone(), hello: hello.clone(), frames, control: control.clone() }) {
        write_frame(&mut send, &refuse(reason)).await?;
        send.finish()?;
        return Ok(());
    }
    write_frame(&mut send, &VideoReply { ok: true, reason: None }).await?;
    // Para empezar a mostrar hace falta un keyframe.
    write_frame(&mut send, &VideoControl::RequestKeyframe).await?;
    let mut waiting_key = true;
    let mut incoming = spawn_reader(wire, conn.clone(), recv, "pantalla del celular");
    loop {
        tokio::select! {
            got = incoming.recv() => {
                let Some(got) = got else { return Ok(()) };
                let frame = match got? {
                    Incoming::Frame(frame) => frame,
                    // El celular ya pidió el keyframe a su codificador.
                    Incoming::Dropped => {
                        waiting_key = true;
                        continue;
                    }
                };
                if waiting_key && !frame.is_key() {
                    continue;
                }
                waiting_key = false;
                match frames_tx.try_send(frame) {
                    Ok(()) => {}
                    // Quien muestra se atrasó: se salta hasta el próximo keyframe.
                    Err(mpsc::error::TrySendError::Full(_)) => {
                        waiting_key = true;
                        write_frame(&mut send, &VideoControl::RequestKeyframe).await?;
                    }
                    // Dejó de mirar.
                    Err(mpsc::error::TrySendError::Closed(_)) => {
                        write_frame(&mut send, &VideoControl::Stop).await.ok();
                        return Ok(());
                    }
                }
            }
            control = control_rx.recv() => {
                let Some(control) = control else { return Ok(()) };
                write_frame(&mut send, &control).await?;
                if control == VideoControl::Stop {
                    send.finish().ok();
                    return Ok(());
                }
            }
        }
    }
}

/// El PC manda su pantalla al celular que la pidió, y le pasa a quien comparte lo que
/// pide el celular (keyframe, bitrate, parar).
async fn share_screen(
    conn: &Connection,
    wire: Wire,
    mut send: SendStream,
    mut recv: RecvStream,
    request: ScreenRequest,
    share: std::result::Result<Arc<ScreenHandler>, String>,
) -> Result<()> {
    let ScreenSource { mut frames, control, max_bitrate } = match share.and_then(|handler| handler(request)) {
        Ok(source) => source,
        Err(reason) => {
            write_frame(&mut send, &VideoReply { ok: false, reason: Some(reason) }).await?;
            send.finish()?;
            send.stopped().await.ok();
            return Ok(());
        }
    };
    write_frame(&mut send, &VideoReply { ok: true, reason: None }).await?;
    // Lo primero que necesita el celular para mostrar algo.
    let _ = control.send(VideoControl::RequestKeyframe);
    // Lo que pide el celular pasa al momento a quien comparte; la tarea termina cuando
    // el celular pide parar o cierra.
    let forward = control.clone();
    let mut asks = tokio::spawn(async move {
        while let Ok(Some(asked)) = read_frame_max::<_, VideoControl>(&mut recv, MAX_VIDEO_MESSAGE).await {
            if asked == VideoControl::Stop {
                break;
            }
            let _ = forward.send(asked);
        }
    });
    // El bitrate que decide v2 le llega a quien comparte como si lo pidiera el celular.
    let adapt = control.clone();
    let on_rate = move |asked: VideoControl| {
        let _ = adapt.send(asked);
    };
    let result = tokio::select! {
        // Quien comparte paró (Ok) o se cortó.
        sent = pump(wire, conn, &mut send, &mut frames, &on_rate, max_bitrate, "pantalla del PC") => sent,
        // El celular dejó de mirar (o pidió parar).
        _ = &mut asks => Ok(()),
    };
    // Avisa a quien comparte que pare.
    let _ = control.send(VideoControl::Stop);
    asks.abort();
    send.finish().ok();
    result
}

// --- Lado del celular ------------------------------------------------------------------

/// Lo que llega del PC mientras el celular manda video.
pub type ControlHandler = dyn Fn(VideoControl) + Send + Sync;

/// Manda cuadros al PC sin acumular: si el envío va atrasado, los descarta hasta el
/// próximo keyframe (y avisa con `RequestKeyframe` al codificador).
pub struct VideoSender {
    frames: mpsc::Sender<VideoFrame>,
    need_key: AtomicBool,
    on_control: Arc<ControlHandler>,
}

impl VideoSender {
    /// `false`: el cuadro se descartó (atrasado) o la sesión terminó.
    pub fn send(&self, flags: u8, pts_us: u64, data: Vec<u8>) -> bool {
        let frame = VideoFrame { flags, pts_us, data };
        if self.need_key.load(Ordering::Relaxed) && !frame.is_key() {
            return false;
        }
        match self.frames.try_send(frame) {
            Ok(()) => {
                self.need_key.store(false, Ordering::Relaxed);
                true
            }
            Err(mpsc::error::TrySendError::Full(frame)) => {
                // El codificador manda un keyframe por pedido: si el descartado era ese, se
                // pide otro (si no, se esperaría al próximo periódico).
                if !self.need_key.swap(true, Ordering::Relaxed) || frame.is_key() {
                    (self.on_control)(VideoControl::RequestKeyframe);
                }
                false
            }
            Err(mpsc::error::TrySendError::Closed(_)) => false,
        }
    }

    pub fn is_closed(&self) -> bool {
        self.frames.is_closed()
    }
}

/// Abre la conexión de video con el PC `desktop` y empieza a mandar. Lo que pida el PC
/// (keyframe, bitrate) llega a `on_control`; al terminar (el PC paró, rechazó o se
/// cortó, o se soltó el `VideoSender`) llega `on_end`, con el motivo si fue un error.
/// Hace falta estar dentro de un runtime de tokio.
pub(crate) fn start_sender(
    endpoint: Endpoint,
    desktop: impl Into<EndpointAddr>,
    hello: VideoHello,
    max_bitrate: u32,
    on_control: Arc<ControlHandler>,
    on_end: Box<dyn FnOnce(Option<String>) + Send>,
) -> VideoSender {
    let (frames_tx, frames) = mpsc::channel(QUEUE);
    let addr = desktop.into();
    let control = on_control.clone();
    tokio::spawn(async move {
        let result = run_sender(endpoint, addr, hello, max_bitrate, frames, control).await;
        on_end(result.err().map(|e| format!("{e:#}")));
    });
    VideoSender { frames: frames_tx, need_key: AtomicBool::new(true), on_control }
}

async fn run_sender(
    endpoint: Endpoint,
    addr: EndpointAddr,
    hello: VideoHello,
    max_bitrate: u32,
    mut frames: mpsc::Receiver<VideoFrame>,
    on_control: Arc<ControlHandler>,
) -> Result<()> {
    let conn = connect(&endpoint, addr).await?;
    let wire = Wire::of(&conn);
    let (mut send, mut recv) = conn.open_bi().await?;
    write_frame(&mut send, &hello).await?;
    let reply: VideoReply = read_frame_max(&mut recv, MAX_VIDEO_MESSAGE).await?.context("El PC cerró sin responder.")?;
    if !reply.ok {
        bail!(reply.reason.unwrap_or_else(|| "El PC no aceptó el video.".into()));
    }
    let mut controls = tokio::spawn(read_controls(recv, on_control.clone()));
    let result = tokio::select! {
        // Se soltó el `VideoSender` (Ok): se terminó de mandar.
        sent = pump(wire, &conn, &mut send, &mut frames, &*on_control, max_bitrate, "pantalla del celular") => sent.context("Se cortó el video."),
        // El PC pidió parar (o cerró).
        _ = &mut controls => Ok(()),
    };
    send.finish().ok();
    controls.abort();
    conn.close(0u32.into(), b"fin");
    result
}

/// Pasa al codificador lo que pide el PC, hasta que pide parar o cierra.
/// La pantalla del PC que mira el celular: pedirle un keyframe u otro bitrate. Soltarla
/// la detiene.
pub struct ScreenView {
    control: mpsc::UnboundedSender<VideoControl>,
}

impl ScreenView {
    pub fn request_keyframe(&self) {
        let _ = self.control.send(VideoControl::RequestKeyframe);
    }

    pub fn set_bitrate(&self, bps: u32) {
        let _ = self.control.send(VideoControl::SetBitrate { bps });
    }

    pub fn is_closed(&self) -> bool {
        self.control.is_closed()
    }
}

impl Drop for ScreenView {
    fn drop(&mut self) {
        let _ = self.control.send(VideoControl::Stop);
    }
}

/// Pide la pantalla del PC `desktop`, a `max_width` de ancho como mucho. Cada cuadro
/// llega a `on_frame` (en orden; con FORMAT al empezar); al terminar llega `on_end`, con
/// el motivo si fue un error o un rechazo. Hace falta estar dentro de un runtime de tokio.
pub(crate) fn start_watcher(
    endpoint: Endpoint,
    desktop: impl Into<EndpointAddr>,
    max_width: u32,
    on_frame: Box<dyn Fn(VideoFrame) + Send + Sync>,
    on_end: Box<dyn FnOnce(Option<String>) + Send>,
) -> ScreenView {
    let (control, control_rx) = mpsc::unbounded_channel();
    let addr = desktop.into();
    tokio::spawn(async move {
        let result = run_watcher(endpoint, addr, max_width, on_frame, control_rx).await;
        on_end(result.err().map(|e| format!("{e:#}")));
    });
    ScreenView { control }
}

async fn run_watcher(
    endpoint: Endpoint,
    addr: EndpointAddr,
    max_width: u32,
    on_frame: Box<dyn Fn(VideoFrame) + Send + Sync>,
    mut control: mpsc::UnboundedReceiver<VideoControl>,
) -> Result<()> {
    let conn = connect(&endpoint, addr).await?;
    let wire = Wire::of(&conn);
    let (mut send, mut recv) = conn.open_bi().await?;
    let hello = VideoHello { source: VideoSource::Screen, direction: VideoDirection::Receive, codec: "h264".into(), width: max_width, height: 0, rotation: 0 };
    write_frame(&mut send, &hello).await?;
    let reply: VideoReply = read_frame_max(&mut recv, MAX_VIDEO_MESSAGE).await?.context("El PC cerró sin responder.")?;
    if !reply.ok {
        bail!(reply.reason.unwrap_or_else(|| "El PC no compartió su pantalla.".into()));
    }
    let mut incoming = spawn_reader(wire, conn.clone(), recv, "pantalla del PC");
    // Tras un cuadro abandonado, el decodificador necesita el keyframe (el PC ya lo pidió).
    let mut waiting_key = false;
    let result = loop {
        tokio::select! {
            got = incoming.recv() => match got {
                Some(Ok(Incoming::Frame(frame))) => {
                    if waiting_key && !frame.is_key() {
                        continue;
                    }
                    waiting_key = false;
                    on_frame(frame);
                }
                Some(Ok(Incoming::Dropped)) => waiting_key = true,
                // El PC dejó de compartir.
                None => break Ok(()),
                Some(Err(e)) => break Err(e.context("Se cortó la pantalla del PC.")),
            },
            asked = control.recv() => {
                // Se soltó la vista: se deja de mirar.
                let asked = asked.unwrap_or(VideoControl::Stop);
                write_frame(&mut send, &asked).await.ok();
                if asked == VideoControl::Stop {
                    break Ok(());
                }
            }
        }
    };
    send.finish().ok();
    conn.close(0u32.into(), b"fin");
    result
}

async fn read_controls(mut recv: RecvStream, on_control: Arc<ControlHandler>) {
    while let Ok(Some(control)) = read_frame_max::<_, VideoControl>(&mut recv, MAX_VIDEO_MESSAGE).await {
        if control == VideoControl::Stop {
            break;
        }
        on_control(control);
    }
}

#[cfg(test)]
#[path = "video_red_mala.rs"]
mod red_mala;

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn un_cuadro_va_y_vuelve_igual() {
        let frame = VideoFrame { flags: flags::KEY | flags::CONFIG, pts_us: 33_366, data: vec![0, 0, 0, 1, 0x67, 0x42] };
        let mut wire = Vec::new();
        write_video_frame(&mut wire, &frame).await.unwrap();
        assert_eq!(wire.len(), 13 + 6);
        let mut reader = wire.as_slice();
        assert_eq!(read_video_frame(&mut reader).await.unwrap(), Some(frame));
        // Corriente terminada limpio.
        assert_eq!(read_video_frame(&mut reader).await.unwrap(), None);
    }

    #[test]
    fn el_formato_viaja_en_su_cuadro() {
        let format = VideoFormat { width: 1080, height: 2340, rotation: 90 };
        let frame = VideoFrame { flags: flags::FORMAT, pts_us: 0, data: serde_json::to_vec(&format).unwrap() };
        assert_eq!(frame.format(), Some(format));
        assert!(frame.is_key());
    }

    #[test]
    fn los_mensajes_se_ven_como_en_la_especificacion() {
        let hello = VideoHello { source: VideoSource::Screen, direction: VideoDirection::Send, codec: "h264".into(), width: 720, height: 1600, rotation: 0 };
        assert_eq!(
            serde_json::to_string(&hello).unwrap(),
            r#"{"source":"screen","direction":"send","codec":"h264","width":720,"height":1600,"rotation":0}"#
        );
        assert_eq!(serde_json::to_string(&VideoControl::SetBitrate { bps: 4_000_000 }).unwrap(), r#"{"type":"set_bitrate","bps":4000000}"#);
        assert_eq!(serde_json::to_string(&VideoControl::RequestKeyframe).unwrap(), r#"{"type":"request_keyframe"}"#);
    }

    /// Un cuadro común de `bytes` que tardó `delay` en confirmarse, entregado en `at`.
    fn delivered(rate: &mut RateControl, at: Instant, delay: Duration, bytes: usize) {
        let flight = Flight { seq: 0, flags: 0, bytes, sent: at - delay, cancel: None };
        rate.on_delivered(&flight, at);
    }

    #[test]
    fn con_cola_el_bitrate_baja_y_sin_cola_vuelve_a_subir() {
        let start = Instant::now();
        let mut rate = RateControl::new(4_000_000, 4_000_000);
        let mut now = start;
        // Medio segundo a 30 cuadros/s, 20 ms de viaje: nada cambia (ya está en el tope).
        for _ in 0..16 {
            now += Duration::from_millis(33);
            delivered(&mut rate, now, Duration::from_millis(20), 16_000);
        }
        assert_eq!(rate.update(now, Duration::ZERO), None);
        // Los cuadros empiezan a tardar 300 ms: hay cola y baja ×0,7.
        for _ in 0..16 {
            now += Duration::from_millis(33);
            delivered(&mut rate, now, Duration::from_millis(300), 16_000);
        }
        assert_eq!(rate.update(now, Duration::ZERO), Some(2_800_000));
        // Sin cola de nuevo y usando el bitrate: pasados 2 s de la bajada, sube ×1,15.
        let mut ups = Vec::new();
        for _ in 0..12 {
            for _ in 0..16 {
                now += Duration::from_millis(33);
                delivered(&mut rate, now, Duration::from_millis(20), 12_000);
            }
            ups.extend(rate.update(now, Duration::ZERO));
        }
        assert_eq!(ups.first(), Some(&3_220_000));
        // Nunca pasa del tope.
        assert_eq!(rate.target, 4_000_000);
    }

    #[test]
    fn un_cuadro_abandonado_baja_a_la_mitad_sin_pasar_del_minimo() {
        let mut rate = RateControl::new(4_000_000, 1_000_000);
        rate.on_stale(Instant::now());
        assert_eq!(rate.target, 500_000);
        rate.on_stale(Instant::now());
        assert_eq!(rate.target, MIN_BITRATE);
    }

    #[test]
    fn con_la_pantalla_quieta_no_sube() {
        // Cuadros chicos (nada cambia): no hay cómo saber si la red da más.
        let mut rate = RateControl::new(4_000_000, 1_000_000);
        let mut now = Instant::now() + Duration::from_secs(5);
        for _ in 0..10 {
            for _ in 0..16 {
                now += Duration::from_millis(33);
                delivered(&mut rate, now, Duration::from_millis(20), 300);
            }
            assert_eq!(rate.update(now, Duration::ZERO), None);
        }
    }

    #[tokio::test]
    async fn atrasado_descarta_hasta_el_proximo_keyframe() {
        let asked = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counter = asked.clone();
        let (frames, mut rx) = mpsc::channel(1);
        let sender = VideoSender {
            frames,
            need_key: AtomicBool::new(true),
            on_control: Arc::new(move |c| {
                if c == VideoControl::RequestKeyframe {
                    counter.fetch_add(1, Ordering::Relaxed);
                }
            }),
        };
        // Sin keyframe todavía: lo demás se descarta.
        assert!(!sender.send(0, 1, vec![1]));
        assert!(sender.send(flags::KEY, 2, vec![2]));
        // La cola (de 1) está llena: se descarta y se pide un keyframe una sola vez.
        assert!(!sender.send(0, 3, vec![3]));
        assert!(!sender.send(0, 4, vec![4]));
        assert_eq!(asked.load(Ordering::Relaxed), 1);
        // Llega el keyframe pedido con la cola todavía llena: se pide otro.
        assert!(!sender.send(flags::KEY, 5, vec![5]));
        assert_eq!(asked.load(Ordering::Relaxed), 2);
        rx.recv().await.unwrap();
        // Hasta el próximo keyframe no pasa nada más.
        assert!(!sender.send(0, 5, vec![5]));
        assert!(sender.send(flags::KEY, 6, vec![6]));
    }
}
