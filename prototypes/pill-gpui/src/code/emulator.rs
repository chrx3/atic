//! La conexión con un emulador de Android por su API gRPC (la misma que usa la vista
//! «Running Devices» de Android Studio): la pantalla llega en cuadros ya escalados al
//! tamaño del panel (`streamScreenshot`, solo cuando cambia) y los toques, las teclas
//! y la rueda van de vuelta. Sin video que decodificar.
//!
//! Los mensajes están escritos a mano (los pocos que se usan, de
//! `emulator/lib/emulator_controller.proto` del SDK): así compilar la pill no pide
//! `protoc`.
//!
//! Todo corre en un hilo con su propio runtime de tokio; la interfaz recibe el último
//! cuadro (`Shared::frame`) y un aviso por canal, y manda la entrada por otro.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::channel::mpsc::UnboundedSender as UiSender;
use tokio::sync::mpsc;
use tonic::codegen::http::uri::PathAndQuery;

const SERVICE: &str = "/android.emulation.control.EmulatorController/";

// --- Mensajes (subconjunto de emulator_controller.proto) ----------------------------

#[derive(Clone, PartialEq, prost::Message)]
pub struct Empty {}

#[derive(Clone, PartialEq, prost::Message)]
pub struct Rotation {
    /// PORTRAIT = 0, LANDSCAPE = 1, REVERSE_PORTRAIT = 2, REVERSE_LANDSCAPE = 3.
    #[prost(int32, tag = "1")]
    pub rotation: i32,
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct ImageFormat {
    /// PNG = 0, RGBA8888 = 1, RGB888 = 2.
    #[prost(int32, tag = "1")]
    pub format: i32,
    #[prost(message, optional, tag = "2")]
    pub rotation: Option<Rotation>,
    #[prost(uint32, tag = "3")]
    pub width: u32,
    #[prost(uint32, tag = "4")]
    pub height: u32,
    #[prost(uint32, tag = "5")]
    pub display: u32,
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct Image {
    #[prost(message, optional, tag = "1")]
    pub format: Option<ImageFormat>,
    #[prost(bytes = "vec", tag = "4")]
    pub image: Vec<u8>,
    #[prost(uint32, tag = "5")]
    pub seq: u32,
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct Touch {
    #[prost(int32, tag = "1")]
    pub x: i32,
    #[prost(int32, tag = "2")]
    pub y: i32,
    #[prost(int32, tag = "3")]
    pub identifier: i32,
    /// 0 levanta el dedo.
    #[prost(int32, tag = "4")]
    pub pressure: i32,
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct TouchEvent {
    #[prost(message, repeated, tag = "1")]
    pub touches: Vec<Touch>,
    #[prost(int32, tag = "2")]
    pub display: i32,
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct KeyboardEvent {
    #[prost(int32, tag = "1")]
    pub code_type: i32,
    /// keydown = 0, keyup = 1, keypress = 2.
    #[prost(int32, tag = "2")]
    pub event_type: i32,
    #[prost(int32, tag = "3")]
    pub key_code: i32,
    /// El nombre de la tecla como en la web (`Enter`, `Backspace`, `GoBack`…).
    #[prost(string, tag = "4")]
    pub key: String,
    #[prost(string, tag = "5")]
    pub text: String,
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct WheelEvent {
    #[prost(int32, tag = "1")]
    pub dx: i32,
    #[prost(int32, tag = "2")]
    pub dy: i32,
    #[prost(int32, tag = "3")]
    pub display: i32,
}

// --- Sesión --------------------------------------------------------------------------

/// Un cuadro listo para pintar: BGRA (lo que espera `gpui::RenderImage`).
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
}

/// Lo que se le manda al emulador.
pub enum Input {
    /// Un dedo en `(x, y)` del dispositivo (en sus píxeles); `down: false` lo levanta.
    Touch { x: i32, y: i32, down: bool },
    /// Una tecla por nombre (`GoBack`, `GoHome`, `AppSwitch`, `Enter`, `Backspace`…).
    Key(String),
    /// Texto escrito.
    Text(String),
    /// La rueda (en «clics»: positivo baja).
    Wheel(i32),
    /// El tamaño al que se piden los cuadros (el del panel, en píxeles de pantalla).
    Size(u32, u32),
}

/// Lo que avisa la sesión a la interfaz.
pub enum Event {
    /// Hay un cuadro nuevo en `Shared::frame`.
    Frame,
    /// Se conectó (o volvió a conectar).
    Connected,
    /// Se cortó la conexión (el emulador se apagó o no contesta).
    Closed(String),
}

#[derive(Default)]
pub struct Shared {
    pub frame: Mutex<Option<Frame>>,
}

/// Una conexión con el emulador que escucha en `port`. Se corta al soltarla.
pub struct Session {
    pub shared: Arc<Shared>,
    input: mpsc::UnboundedSender<Input>,
    _thread: std::thread::JoinHandle<()>,
}

impl Session {
    pub fn connect(port: u16, size: (u32, u32), events: UiSender<Event>) -> Self {
        let shared = Arc::new(Shared::default());
        let (input, inbox) = mpsc::unbounded_channel();
        let thread_shared = shared.clone();
        let thread = std::thread::Builder::new()
            .name("atic-emulador".into())
            .spawn(move || {
                let runtime = match tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build() {
                    Ok(runtime) => runtime,
                    Err(error) => {
                        let _ = events.unbounded_send(Event::Closed(error.to_string()));
                        return;
                    }
                };
                runtime.block_on(run(port, size, thread_shared, inbox, events));
            })
            .expect("hilo del emulador");
        Self { shared, input, _thread: thread }
    }

    pub fn send(&self, input: Input) {
        let _ = self.input.send(input);
    }
}

type Grpc = tonic::client::Grpc<tonic::transport::Channel>;

async fn channel(port: u16) -> Result<Grpc, String> {
    let endpoint = tonic::transport::Endpoint::from_shared(format!("http://127.0.0.1:{port}"))
        .map_err(|e| e.to_string())?
        .connect_timeout(Duration::from_secs(3))
        // Cuadros grandes: el límite de 4 MB de tonic no alcanza para uno sin escalar.
        .initial_stream_window_size(Some(8 << 20));
    let channel = endpoint.connect().await.map_err(|e| e.to_string())?;
    Ok(tonic::client::Grpc::new(channel).max_decoding_message_size(64 << 20))
}

async fn unary<Q: prost::Message + 'static, R: prost::Message + Default + 'static>(grpc: &mut Grpc, method: &'static str, request: Q) -> Result<R, String> {
    grpc.ready().await.map_err(|e| e.to_string())?;
    let path = PathAndQuery::try_from(format!("{SERVICE}{method}")).map_err(|e| e.to_string())?;
    let codec = tonic::codec::ProstCodec::<Q, R>::default();
    grpc.unary(tonic::Request::new(request), path, codec).await.map(|r| r.into_inner()).map_err(|e| e.message().to_string())
}

/// La sesión: el flujo de cuadros (se rehace al cambiar el tamaño) y la entrada.
async fn run(port: u16, size: (u32, u32), shared: Arc<Shared>, mut inbox: mpsc::UnboundedReceiver<Input>, events: UiSender<Event>) {
    let mut grpc = match channel(port).await {
        Ok(grpc) => grpc,
        Err(error) => {
            let _ = events.unbounded_send(Event::Closed(error));
            return;
        }
    };
    let _ = events.unbounded_send(Event::Connected);
    let mut size = size;
    loop {
        let stream = tokio::spawn(stream_frames(grpc.clone(), size, shared.clone(), events.clone()));
        // Hasta que cambie el tamaño (otro flujo) o se suelte la sesión.
        let resize = loop {
            let Some(input) = inbox.recv().await else {
                stream.abort();
                return;
            };
            match input {
                Input::Size(w, h) if (w, h) != size => break (w, h),
                Input::Size(..) => {}
                other => {
                    if let Err(error) = send_input(&mut grpc, other).await {
                        log::warn!("emulador: entrada perdida: {error}");
                    }
                }
            }
            if stream.is_finished() {
                break size;
            }
        };
        stream.abort();
        if stream.await.is_ok() && resize == size {
            // El flujo terminó solo: el emulador se fue.
            let _ = events.unbounded_send(Event::Closed(String::new()));
            return;
        }
        size = resize;
    }
}

async fn stream_frames(mut grpc: Grpc, (width, height): (u32, u32), shared: Arc<Shared>, events: UiSender<Event>) {
    let request = ImageFormat { format: 2, rotation: None, width, height, display: 0 };
    let result: Result<(), String> = async {
        grpc.ready().await.map_err(|e| e.to_string())?;
        let path = PathAndQuery::try_from(format!("{SERVICE}streamScreenshot")).map_err(|e| e.to_string())?;
        let codec = tonic::codec::ProstCodec::<ImageFormat, Image>::default();
        let mut stream = grpc.server_streaming(tonic::Request::new(request), path, codec).await.map_err(|e| e.message().to_string())?.into_inner();
        while let Some(image) = stream.message().await.map_err(|e| e.message().to_string())? {
            let Some(format) = image.format.as_ref() else {
                continue;
            };
            // Pantalla apagada o todavía arrancando: llega vacío.
            if format.width == 0 || format.height == 0 || image.image.len() < (format.width * format.height * 3) as usize {
                continue;
            }
            let frame = Frame { width: format.width, height: format.height, bgra: rgb_to_bgra(&image.image) };
            *shared.frame.lock().unwrap_or_else(|e| e.into_inner()) = Some(frame);
            if events.unbounded_send(Event::Frame).is_err() {
                break;
            }
        }
        Ok(())
    }
    .await;
    if let Err(error) = result {
        let _ = events.unbounded_send(Event::Closed(error));
    }
}

async fn send_input(grpc: &mut Grpc, input: Input) -> Result<(), String> {
    match input {
        Input::Touch { x, y, down } => {
            let touch = Touch { x, y, identifier: 0, pressure: if down { 1 } else { 0 } };
            unary::<_, Empty>(grpc, "sendTouch", TouchEvent { touches: vec![touch], display: 0 }).await?;
        }
        Input::Key(key) => {
            unary::<_, Empty>(grpc, "sendKey", KeyboardEvent { event_type: 2, key, ..Default::default() }).await?;
        }
        Input::Text(text) => {
            unary::<_, Empty>(grpc, "sendKey", KeyboardEvent { event_type: 2, text, ..Default::default() }).await?;
        }
        Input::Wheel(dy) => {
            grpc.ready().await.map_err(|e| e.to_string())?;
            let path = PathAndQuery::try_from(format!("{SERVICE}injectWheel")).map_err(|e| e.to_string())?;
            let codec = tonic::codec::ProstCodec::<WheelEvent, Empty>::default();
            let events = futures::stream::iter([WheelEvent { dx: 0, dy, display: 0 }]);
            grpc.client_streaming(tonic::Request::new(events), path, codec).await.map_err(|e| e.message().to_string())?;
        }
        Input::Size(..) => {}
    }
    Ok(())
}

/// RGB888 (lo que manda el emulador) a BGRA (lo que pinta GPUI).
pub fn rgb_to_bgra(rgb: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(rgb.len() / 3 * 4);
    for px in rgb.chunks_exact(3) {
        out.extend_from_slice(&[px[2], px[1], px[0], 255]);
    }
    out
}

/// Un punto del cuadro mostrado (`at`, en el rectángulo `shown` de `w × h`) en píxeles
/// del dispositivo (`device`), o `None` si cae fuera.
pub fn to_device(at: (f32, f32), shown: (f32, f32), device: (u32, u32)) -> Option<(i32, i32)> {
    let (x, y) = at;
    let (w, h) = shown;
    if w <= 0. || h <= 0. || x < 0. || y < 0. || x > w || y > h {
        return None;
    }
    Some(((x / w * device.0 as f32).round() as i32, (y / h * device.1 as f32).round() as i32))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn los_colores_pasan_a_bgra() {
        assert_eq!(rgb_to_bgra(&[10, 20, 30, 40, 50, 60]), vec![30, 20, 10, 255, 60, 50, 40, 255]);
    }

    #[test]
    fn un_clic_en_el_cuadro_cae_en_su_pixel_del_telefono() {
        // Un cuadro de 270×600 mostrando un teléfono de 1080×2400.
        assert_eq!(to_device((135., 300.), (270., 600.), (1080, 2400)), Some((540, 1200)));
        assert_eq!(to_device((0., 0.), (270., 600.), (1080, 2400)), Some((0, 0)));
        assert_eq!(to_device((271., 10.), (270., 600.), (1080, 2400)), None);
    }

    /// Contra un emulador encendido con `-grpc <puerto>` (`ATIC_EMU_PORT`): a mano.
    #[test]
    #[ignore]
    fn recibe_cuadros_de_un_emulador_encendido() {
        let port: u16 = std::env::var("ATIC_EMU_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(8554);
        let (events, mut inbox) = futures::channel::mpsc::unbounded();
        let session = Session::connect(port, (360, 800), events);
        let started = std::time::Instant::now();
        let mut frames = 0;
        while started.elapsed() < Duration::from_secs(8) {
            match inbox.try_next() {
                Ok(Some(Event::Frame)) => frames += 1,
                Ok(Some(Event::Closed(error))) => panic!("se cortó: {error}"),
                _ => std::thread::sleep(Duration::from_millis(10)),
            }
            if frames == 1 {
                session.send(Input::Key("GoHome".into()));
            }
        }
        let frame = session.shared.frame.lock().unwrap().take().expect("al menos un cuadro");
        println!("{frames} cuadros; el último de {}×{}", frame.width, frame.height);
        assert!(frame.width <= 360 && frame.height <= 800 && frame.width > 0);
    }

    #[test]
    fn los_mensajes_se_codifican_como_el_proto() {
        use prost::Message;
        // `width = 3` (varint 320) y `height = 4` (varint 640), formato RGB888.
        let bytes = ImageFormat { format: 2, rotation: None, width: 320, height: 640, display: 0 }.encode_to_vec();
        assert_eq!(bytes, vec![0x08, 0x02, 0x18, 0xc0, 0x02, 0x20, 0x80, 0x05]);
    }
}
