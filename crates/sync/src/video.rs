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

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use iroh::endpoint::{Connection, RecvStream};
use iroh::{Endpoint, EndpointAddr};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;

use crate::proto::{read_frame_max, write_frame};

/// El ALPN del video: un cambio incompatible cambia el número.
pub const VIDEO_ALPN: &[u8] = b"atic/video/1";
/// Tope de un cuadro (un keyframe 4K en H.264 cabe de sobra).
pub const MAX_VIDEO_FRAME: usize = 4 << 20;
/// Tope de un mensaje JSON del video (saludo, formato, control).
const MAX_VIDEO_MESSAGE: u32 = 16 * 1024;
/// Cuadros en espera a cada lado: más, y el video se atrasa en vez de saltar.
const QUEUE: usize = 4;

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

/// Atiende una conexión `atic/video/1` en el PC. `allowed`: el equipo está pareado y la
/// función prendida (lo sabe `desktop.rs`).
pub(crate) async fn serve_desktop(conn: Connection, allowed: std::result::Result<(), String>, handler: Option<Arc<VideoHandler>>) -> Result<()> {
    let device_id = conn.remote_id().to_string();
    let (mut send, mut recv) = conn.accept_bi().await?;
    let hello: VideoHello = read_frame_max(&mut recv, MAX_VIDEO_MESSAGE).await?.context("cerró sin saludar")?;
    let refuse = |reason: String| VideoReply { ok: false, reason: Some(reason) };
    let decided = allowed
        .and_then(|()| match (hello.direction, hello.codec.as_str()) {
            (VideoDirection::Send, "h264") => Ok(()),
            (VideoDirection::Receive, _) => Err("El PC todavía no comparte su pantalla.".into()),
            (_, codec) => Err(format!("Formato de video no soportado: {codec}")),
        })
        .and_then(|()| handler.ok_or_else(|| "Atic no está mostrando video ahora.".to_string()));
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
    loop {
        tokio::select! {
            frame = read_video_frame(&mut recv) => {
                let Some(frame) = frame? else { return Ok(()) };
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
            Err(mpsc::error::TrySendError::Full(_)) => {
                if !self.need_key.swap(true, Ordering::Relaxed) {
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
    on_control: Arc<ControlHandler>,
    on_end: Box<dyn FnOnce(Option<String>) + Send>,
) -> VideoSender {
    let (frames_tx, frames) = mpsc::channel(QUEUE);
    let addr = desktop.into();
    let control = on_control.clone();
    tokio::spawn(async move {
        let result = run_sender(endpoint, addr, hello, frames, control).await;
        on_end(result.err().map(|e| format!("{e:#}")));
    });
    VideoSender { frames: frames_tx, need_key: AtomicBool::new(true), on_control }
}

async fn run_sender(endpoint: Endpoint, addr: EndpointAddr, hello: VideoHello, mut frames: mpsc::Receiver<VideoFrame>, on_control: Arc<ControlHandler>) -> Result<()> {
    let conn = endpoint.connect(addr, VIDEO_ALPN).await.map_err(|e| anyhow::anyhow!("No se pudo llegar al PC: {e:#}"))?;
    let (mut send, mut recv) = conn.open_bi().await?;
    write_frame(&mut send, &hello).await?;
    let reply: VideoReply = read_frame_max(&mut recv, MAX_VIDEO_MESSAGE).await?.context("El PC cerró sin responder.")?;
    if !reply.ok {
        bail!(reply.reason.unwrap_or_else(|| "El PC no aceptó el video.".into()));
    }
    let mut controls = tokio::spawn(read_controls(recv, on_control));
    let result = loop {
        tokio::select! {
            frame = frames.recv() => {
                // Se soltó el `VideoSender`: se terminó de mandar.
                let Some(frame) = frame else { break Ok(()) };
                if let Err(e) = write_video_frame(&mut send, &frame).await {
                    break Err(e.context("Se cortó el video."));
                }
            }
            // El PC pidió parar (o cerró).
            _ = &mut controls => break Ok(()),
        }
    };
    send.finish().ok();
    controls.abort();
    conn.close(0u32.into(), b"fin");
    result
}

/// Pasa al codificador lo que pide el PC, hasta que pide parar o cierra.
async fn read_controls(mut recv: RecvStream, on_control: Arc<ControlHandler>) {
    while let Ok(Some(control)) = read_frame_max::<_, VideoControl>(&mut recv, MAX_VIDEO_MESSAGE).await {
        if control == VideoControl::Stop {
            break;
        }
        on_control(control);
    }
}

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
        rx.recv().await.unwrap();
        // Hasta el próximo keyframe no pasa nada más.
        assert!(!sender.send(0, 5, vec![5]));
        assert!(sender.send(flags::KEY, 6, vec![6]));
    }
}
