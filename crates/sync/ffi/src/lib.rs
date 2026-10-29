//! El lado celular de `atic-sync`, para Kotlin.
//!
//! Un celular puede tener varios PCs: cada evento dice de cuál viene
//! (`desktop_id`, la clave pública del PC) y lo que va a uno en particular lo
//! lleva como primer argumento.
//!
//! [`AticPhone`] tiene su propio runtime de tokio: Kotlin llama métodos
//! comunes (no suspend) y recibe todo por [`PhoneListener`], desde un hilo del
//! runtime. Quien implementa el listener pasa los datos a su hilo principal.

use std::sync::Arc;

use atic_sync::{
    phone::{LinkStatus, PairedDesktop, Phone, PhoneConfig, PhoneEvent, Target},
    AgentAction, AgentCard, AgentEntry, CallAction, CallState, ClipItem, Decision, FileHeader, FileKind, InputEvent, MediaState, PcCommand, PcState,
    PhoneNotice, PromptImage,
};
use tokio::sync::mpsc::unbounded_channel;

uniffi::setup_scaffolding!();

#[derive(Debug, thiserror::Error, uniffi::Error)]
#[uniffi(flat_error)]
pub enum SyncError {
    #[error("{0}")]
    Failed(String),
}

impl From<anyhow::Error> for SyncError {
    fn from(e: anyhow::Error) -> Self {
        SyncError::Failed(format!("{e:#}"))
    }
}

impl From<std::io::Error> for SyncError {
    fn from(e: std::io::Error) -> Self {
        SyncError::Failed(e.to_string())
    }
}

#[derive(Debug, Clone, uniffi::Enum)]
pub enum LinkState {
    Connecting,
    /// El ticket era bueno; falta que alguien acepte en el PC.
    AwaitingApproval,
    /// `features`: lo que sabe hacer ese PC (`files`, `notices`…). Un PC viejo, ninguna.
    Connected { desktop_name: String, features: Vec<String> },
    /// Se reintenta solo, salvo después de [`PhoneListener::on_rejected`].
    Offline { reason: String },
}

/// El bitrate con que parte el codificador de la app (`ScreenShareService.BITRATE`):
/// la conexión lo baja si la red no da y no pide más que esto.
const SCREEN_BITRATE: u32 = 4_000_000;

/// Lo que pide el PC mientras el celular le manda la pantalla.
#[uniffi::export(with_foreign)]
pub trait VideoListener: Send + Sync {
    /// Mandar un keyframe ya (`MediaCodec.PARAMETER_KEY_REQUEST_SYNC_FRAME`).
    fn on_request_keyframe(&self);
    /// Bitrate nuevo (`MediaCodec.PARAMETER_KEY_VIDEO_BITRATE`).
    fn on_set_bitrate(&self, bps: u32);
    /// Terminó: el PC paró, rechazó o se cortó. `reason` si fue un error o un rechazo.
    fn on_stop(&self, reason: Option<String>);
}

/// La pantalla del PC que mira el celular.
#[uniffi::export(with_foreign)]
pub trait ScreenWatcher: Send + Sync {
    /// Un cuadro, en orden: `flags` bit0 keyframe, bit1 config (SPS/PPS), bit2 formato (los
    /// datos son el JSON `{width,height,rotation}`); los demás, H.264 Annex-B para MediaCodec.
    fn on_frame(&self, flags: u8, pts_us: u64, data: Vec<u8>);
    /// Terminó: el PC dejó de compartir, rechazó o se cortó. `reason` si fue un error.
    fn on_stop(&self, reason: Option<String>);
}

/// La pantalla de un PC que se está mirando. Soltarla la detiene.
#[derive(uniffi::Object)]
pub struct AticScreenView {
    view: atic_sync::video::ScreenView,
}

#[uniffi::export]
impl AticScreenView {
    /// Pedir un keyframe (después de un hueco o al volver a la vista).
    pub fn request_keyframe(&self) {
        self.view.request_keyframe();
    }

    pub fn set_bitrate(&self, bps: u32) {
        self.view.set_bitrate(bps);
    }

    pub fn is_closed(&self) -> bool {
        self.view.is_closed()
    }
}

/// Una pantalla que se está mandando al PC. Soltarla la detiene.
#[derive(uniffi::Object)]
pub struct AticVideo {
    sender: atic_sync::video::VideoSender,
}

#[uniffi::export]
impl AticVideo {
    /// Un buffer de MediaCodec: `flags` bit0 keyframe, bit1 config (csd), bit2 formato
    /// (los datos son el JSON `{width,height,rotation}`); `pts_us` desde el inicio. `false`
    /// si se descartó (va atrasado: esperar el keyframe que se pidió) o terminó.
    pub fn send(&self, flags: u8, pts_us: u64, data: Vec<u8>) -> bool {
        self.sender.send(flags, pts_us, data)
    }

    pub fn is_closed(&self) -> bool {
        self.sender.is_closed()
    }
}

#[uniffi::export(with_foreign)]
pub trait PhoneListener: Send + Sync {
    /// Foto completa de los agentes de ese PC.
    fn on_agents(&self, desktop_id: String, agents: Vec<AgentCard>);
    fn on_link(&self, desktop_id: String, state: LinkState);
    /// El ticket funcionó. Guardar `desktop_json` y pasarlo a
    /// [`AticPhone::connect`] en los próximos arranques.
    fn on_paired(&self, desktop_id: String, desktop_json: String, desktop_name: String);
    /// El PC no acepta a este celular; no se reintenta.
    fn on_rejected(&self, desktop_id: String, reason: String);
    /// Algo que se copió en ese PC.
    fn on_clip(&self, desktop_id: String, item: ClipItem);
    /// Lo que suena en ese PC y si está grabando.
    fn on_pc(&self, desktop_id: String, pc: PcState);
    /// Historial del portapapeles de ese PC (vacío si está apagado).
    fn on_clip_history(&self, desktop_id: String, items: Vec<ClipItem>);
    /// La imagen pedida con `fetch_image`. Sin `data`: ya no está en el PC.
    fn on_image(&self, desktop_id: String, id: String, mime: String, data: Option<Vec<u8>>);
    /// La pill de ese PC pide play/pausa, siguiente o anterior.
    fn on_media_command(&self, desktop_id: String, command: PcCommand);
    /// Ese PC quiere estas imágenes del historial del celular (`send_clip_image`).
    fn on_clip_need(&self, desktop_id: String, ids: Vec<String>);
    /// Se borraron en ese PC: borrarlas acá y pasarlo a los otros PCs.
    fn on_clip_deleted(&self, desktop_id: String, ids: Vec<String>);
    /// Contestar una notificación del celular desde el PC.
    fn on_notice_reply(&self, desktop_id: String, key: String, text: String);
    /// Descartarla desde el PC.
    fn on_notice_dismiss(&self, desktop_id: String, key: String);
    /// Contestar o colgar la llamada desde el PC.
    fn on_call_action(&self, desktop_id: String, action: CallAction);
    /// Ese PC quiere encontrar el celular: sonar fuerte; `false`, callarse.
    fn on_ring(&self, desktop_id: String, on: bool);
    /// La respuesta de ese PC a `request_access`. Si se aceptó, antes llega
    /// `on_link` conectado con las funciones nuevas.
    fn on_access_answer(&self, desktop_id: String, features: Vec<String>, granted: bool);
    /// Abrir este enlace de ese PC en el navegador (solo `http`/`https`).
    fn on_open_url(&self, desktop_id: String, url: String);
    /// Ese PC pide una foto: abrir la cámara y contestar con `send_photo`.
    /// Si se cancela, no se manda nada.
    fn on_take_photo(&self, desktop_id: String);
    /// Lo nuevo que escribió el agente que se mira, por partes (lo del usuario,
    /// la respuesta en Markdown, herramientas) y con imágenes; todo si `reset`.
    /// Con un PC que anuncia "agents" llega esto en vez de `on_agent_output`.
    fn on_agent_entries(&self, desktop_id: String, agent_id: String, entries: Vec<AgentEntry>, reset: bool);
    /// Lo nuevo que escribió el agente que se mira; todo si `reset`.
    fn on_agent_output(&self, desktop_id: String, agent_id: String, lines: Vec<String>, reset: bool);
    /// Un archivo del PC, ya guardado en la carpeta de entrada (`path`).
    fn on_file_received(&self, desktop_id: String, header: FileHeader, path: String);
    /// Avance de un archivo que se manda; `done` al terminar, `error` si falló.
    fn on_file_progress(&self, desktop_id: String, id: String, sent: u64, total: u64);
    fn on_file_done(&self, desktop_id: String, id: String, error: Option<String>);
}

/// Si se puede mandar como enlace: `http` o `https`, sin espacios ni
/// caracteres raros (lo mismo que revisan `open_url` y el PC).
#[uniffi::export]
pub fn is_web_url(url: String) -> bool {
    atic_sync::is_web_url(&url)
}

/// El id de un PC guardado (lo que llegó en `on_paired`).
#[uniffi::export]
pub fn desktop_id(desktop_json: String) -> Result<String, SyncError> {
    Ok(PairedDesktop::from_json(&desktop_json)?.id())
}

/// Clave nueva para este celular. Se guarda en almacenamiento privado de la app.
#[uniffi::export]
pub fn generate_secret_key() -> Vec<u8> {
    atic_sync::generate_secret_key().to_vec()
}

#[derive(uniffi::Object)]
pub struct AticPhone {
    // El orden importa al soltar: primero el teléfono (cierra tareas), después el runtime.
    phone: Phone,
    listener: Arc<dyn PhoneListener>,
    runtime: tokio::runtime::Runtime,
}

#[uniffi::export]
impl AticPhone {
    #[uniffi::constructor]
    /// `inbox_dir`: dónde quedan los archivos que mandan los PCs.
    pub fn new(
        secret_key: Vec<u8>,
        device_name: String,
        inbox_dir: Option<String>,
        listener: Arc<dyn PhoneListener>,
    ) -> Result<Arc<Self>, SyncError> {
        let secret_key: [u8; 32] = secret_key
            .try_into()
            .map_err(|_| SyncError::Failed("la clave debe tener 32 bytes".into()))?;
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("atic-sync")
            .enable_all()
            .build()?;
        let phone = runtime.block_on(Phone::start(PhoneConfig { secret_key, device_name, inbox: inbox_dir.map(Into::into) }))?;
        Ok(Arc::new(Self { phone, listener, runtime }))
    }

    /// Parea con el texto del QR (`atic1:...`) y queda conectado. Devuelve el
    /// id del PC. Los otros PCs siguen conectados.
    pub fn pair(&self, ticket: String) -> Result<String, SyncError> {
        let target = Target::from_ticket(&ticket)?;
        Ok(self.start(target))
    }

    /// Se conecta a un PC ya pareado (lo que llegó en `on_paired`). Devuelve su id.
    pub fn connect(&self, desktop_json: String) -> Result<String, SyncError> {
        let desktop = PairedDesktop::from_json(&desktop_json)?;
        Ok(self.start(Target::Known(desktop)))
    }

    /// Los PCs con sesión abierta ahora.
    pub fn connected(&self) -> Vec<String> {
        self.phone.connected()
    }

    /// `false` si ese PC no está conectado: la decisión no salió.
    pub fn decide(&self, desktop_id: String, agent_id: String, permission_id: String, decision: Decision) -> bool {
        self.phone.decide(&desktop_id, agent_id, permission_id, decision)
    }

    /// Contesta una pregunta del agente: una respuesta por pregunta, en orden.
    pub fn answer(&self, desktop_id: String, agent_id: String, permission_id: String, answers: Vec<String>) -> bool {
        self.phone.answer(&desktop_id, agent_id, permission_id, answers)
    }

    /// Ítems del historial del celular y lo borrado, para que ese PC sume.
    pub fn sync_clips(&self, desktop_id: String, items: Vec<ClipItem>, deleted: Vec<String>) -> bool {
        self.phone.sync_clips(&desktop_id, items, deleted)
    }

    pub fn send_clip_image(&self, desktop_id: String, id: String, mime: String, data: Vec<u8>) -> bool {
        self.phone.send_clip_image(&desktop_id, id, mime, data)
    }

    /// Borrados a todos los PCs conectados.
    pub fn delete_clips(&self, ids: Vec<String>) -> bool {
        self.phone.delete_clips(ids)
    }

    /// Borrados a un PC (los que trajo otro).
    pub fn delete_clips_on(&self, desktop_id: String, ids: Vec<String>) -> bool {
        self.phone.delete_clips_on(&desktop_id, ids)
    }

    /// Lo que suena en el celular, para la pill de los PCs. `None`: nada.
    pub fn publish_media(&self, media: Option<MediaState>, art: Option<Vec<u8>>) -> bool {
        self.phone.publish_media(media, art)
    }

    /// Pide a ese PC la imagen entera de un ítem de su historial.
    pub fn fetch_image(&self, desktop_id: String, id: String) -> bool {
        self.phone.fetch_image(&desktop_id, id)
    }

    /// Manda una imagen (PNG o JPEG) al portapapeles de los PCs conectados.
    pub fn send_image(&self, mime: String, data: Vec<u8>) -> bool {
        self.phone.send_image(mime, data)
    }

    /// Abre un enlace en el navegador de ese PC. `false` si no sabe (PC viejo)
    /// o no es `http`/`https`.
    pub fn open_url(&self, desktop_id: String, url: String) -> bool {
        self.phone.open_url(&desktop_id, url)
    }

    /// Pide a ese PC prender funciones apagadas ("prompt", "control", "remote").
    /// En el PC aparece la pregunta; contesta con `on_access_answer`. `false`
    /// si ese PC no sabe recibir el pedido (no anuncia "access").
    pub fn request_access(&self, desktop_id: String, features: Vec<String>) -> bool {
        self.phone.request_access(&desktop_id, features)
    }

    /// La foto que pidió ese PC (`on_take_photo`), en JPEG o PNG de hasta 5 MB.
    pub fn send_photo(&self, desktop_id: String, mime: String, data: Vec<u8>) -> bool {
        self.phone.send_photo(&desktop_id, mime, data)
    }

    /// Medios o detener la grabación de ese PC. `false` si no está conectado.
    pub fn command(&self, desktop_id: String, command: PcCommand) -> bool {
        self.phone.command(&desktop_id, command)
    }

    /// El puntero o el teclado de ese PC. `false` si no sabe recibirlo.
    pub fn input(&self, desktop_id: String, event: InputEvent) -> bool {
        self.phone.input(&desktop_id, event)
    }

    /// Manda texto al portapapeles de los PCs conectados. `false` si no hay
    /// ninguno o es demasiado largo.
    pub fn send_clip(&self, text: String, copied_at_ms: i64) -> bool {
        self.phone.send_clip(text, copied_at_ms)
    }

    /// Llamarlo cuando Android avisa un cambio de red (`NetworkCallback`): iroh
    /// busca caminos nuevos y los PCs desconectados reintentan ya.
    pub fn network_changed(&self) {
        let _guard = self.runtime.enter();
        self.phone.network_change();
    }

    /// Lo que sabe hacer ese PC ahora (vacío sin conexión).
    pub fn features(&self, desktop_id: String) -> Vec<String> {
        self.phone.features(&desktop_id)
    }

    /// Una notificación del celular, a los PCs que las muestran. A cuántos salió.
    pub fn publish_notice(&self, notice: PhoneNotice) -> u32 {
        self.phone.publish_notice(notice) as u32
    }

    pub fn notice_removed(&self, key: String) -> u32 {
        self.phone.notice_removed(key) as u32
    }

    /// La llamada en curso (`None`: terminó), a los PCs que las muestran.
    pub fn publish_call(&self, call: Option<CallState>) -> u32 {
        self.phone.publish_call(call) as u32
    }

    /// Le escribe a un agente de ese PC.
    pub fn prompt(&self, desktop_id: String, agent_id: String, text: String) -> bool {
        self.phone.prompt(&desktop_id, agent_id, text)
    }

    /// Un mensaje con imágenes: del celular (`images`, hasta 4 y 5 MB en total;
    /// JPEG, PNG, WebP o GIF) o del historial del portapapeles de ese PC
    /// (`clip_ids`). Sin texto vale si lleva alguna. `false` si ese PC no las
    /// recibe (no anuncia "agents") o no caben.
    pub fn prompt_with(&self, desktop_id: String, agent_id: String, text: String, images: Vec<PromptImage>, clip_ids: Vec<String>) -> bool {
        self.phone.prompt_with(&desktop_id, agent_id, text, images, clip_ids)
    }

    /// Aceptar (queda revisado), abrir en el PC o detener un agente. `false` si
    /// ese PC no lo sabe (no anuncia "agents"; detener pide además "prompt").
    pub fn agent_action(&self, desktop_id: String, agent_id: String, action: AgentAction) -> bool {
        self.phone.agent_action(&desktop_id, agent_id, action)
    }

    /// Mirar en vivo un agente de ese PC; `None` para dejar de mirar.
    pub fn watch(&self, desktop_id: String, agent_id: Option<String>) -> bool {
        self.phone.watch(&desktop_id, agent_id)
    }

    /// Manda un archivo a ese PC. Devuelve el id del envío (el avance llega por
    /// `on_file_progress` y `on_file_done`), o `None` si ese PC no recibe archivos.
    pub fn send_file(
        &self,
        desktop_id: String,
        path: String,
        name: String,
        mime: String,
        kind: FileKind,
        meta: Option<String>,
    ) -> Option<String> {
        let _guard = self.runtime.enter();
        self.phone.send_file(&desktop_id, std::path::Path::new(&path), name, mime, kind, meta)
    }

    /// Empieza a mandar la pantalla a ese PC (lo que entrega MediaCodec en H.264, por
    /// [`AticVideo::send`]). `None` si ese PC no está conectado o no muestra video. Lo
    /// que pide el PC y el final llegan a `listener`. Para parar, soltar el `AticVideo`.
    pub fn start_screen(&self, desktop_id: String, width: u32, height: u32, rotation: u32, listener: Arc<dyn VideoListener>) -> Option<Arc<AticVideo>> {
        use atic_sync::video::{VideoControl, VideoDirection, VideoHello, VideoSource};
        let _guard = self.runtime.enter();
        let hello = VideoHello { source: VideoSource::Screen, direction: VideoDirection::Send, codec: "h264".into(), width, height, rotation };
        let control = listener.clone();
        let sender = self.phone.start_video(
            &desktop_id,
            hello,
            SCREEN_BITRATE,
            move |c| match c {
                VideoControl::RequestKeyframe => control.on_request_keyframe(),
                VideoControl::SetBitrate { bps } => control.on_set_bitrate(bps),
                VideoControl::Stop => {}
            },
            move |reason| listener.on_stop(reason),
        )?;
        Some(Arc::new(AticVideo { sender }))
    }

    /// Mira la pantalla de ese PC, a `max_width` de ancho como mucho. Los cuadros y el
    /// final llegan a `listener`. `None` si ese PC no está conectado o no comparte su
    /// pantalla. Para dejar de mirar, soltar el `AticScreenView`.
    pub fn watch_screen(&self, desktop_id: String, max_width: u32, listener: Arc<dyn ScreenWatcher>) -> Option<Arc<AticScreenView>> {
        let _guard = self.runtime.enter();
        let frames = listener.clone();
        let view = self.phone.watch_screen(
            &desktop_id,
            max_width,
            move |frame| frames.on_frame(frame.flags, frame.pts_us, frame.data),
            move |reason| listener.on_stop(reason),
        )?;
        Some(Arc::new(AticScreenView { view }))
    }

    pub fn disconnect(&self, desktop_id: String) {
        self.phone.disconnect(&desktop_id);
    }

    pub fn disconnect_all(&self) {
        self.phone.disconnect_all();
    }
}

impl AticPhone {
    fn start(&self, target: Target) -> String {
        let _guard = self.runtime.enter();
        let (tx, mut rx) = unbounded_channel();
        let id = self.phone.connect(target, tx);
        let listener = self.listener.clone();
        let desktop = id.clone();
        self.runtime.spawn(async move {
            while let Some(event) = rx.recv().await {
                let d = desktop.clone();
                match event {
                    PhoneEvent::Agents(agents) => listener.on_agents(d, agents),
                    PhoneEvent::Status(status) => listener.on_link(
                        d,
                        match status {
                            LinkStatus::Connecting => LinkState::Connecting,
                            LinkStatus::AwaitingApproval => LinkState::AwaitingApproval,
                            LinkStatus::Connected { desktop_name, features } => {
                                LinkState::Connected { desktop_name, features }
                            }
                            LinkStatus::Offline { reason } => LinkState::Offline { reason },
                        },
                    ),
                    PhoneEvent::Paired(desktop) => listener.on_paired(d, desktop.to_json(), desktop.name),
                    PhoneEvent::Rejected(reason) => listener.on_rejected(d, reason),
                    PhoneEvent::Clip(item) => listener.on_clip(d, item),
                    PhoneEvent::Pc(pc) => listener.on_pc(d, pc),
                    PhoneEvent::ClipHistory(items) => listener.on_clip_history(d, items),
                    PhoneEvent::Image { id, mime, data } => listener.on_image(d, id, mime, data),
                    PhoneEvent::MediaCommand(command) => listener.on_media_command(d, command),
                    PhoneEvent::ClipNeed(ids) => listener.on_clip_need(d, ids),
                    PhoneEvent::ClipDeleted(ids) => listener.on_clip_deleted(d, ids),
                    PhoneEvent::NoticeReply { key, text } => listener.on_notice_reply(d, key, text),
                    PhoneEvent::NoticeDismiss { key } => listener.on_notice_dismiss(d, key),
                    PhoneEvent::CallAction(action) => listener.on_call_action(d, action),
                    PhoneEvent::Ring(on) => listener.on_ring(d, on),
                    PhoneEvent::OpenUrl(url) => listener.on_open_url(d, url),
                    PhoneEvent::TakePhoto => listener.on_take_photo(d),
                    PhoneEvent::AccessAnswer { features, granted } => listener.on_access_answer(d, features, granted),
                    PhoneEvent::AgentEntries { agent_id, entries, reset } => listener.on_agent_entries(d, agent_id, entries, reset),
                    PhoneEvent::AgentOutput { agent_id, lines, reset } => {
                        listener.on_agent_output(d, agent_id, lines, reset)
                    }
                    PhoneEvent::FileReceived { header, path } => {
                        listener.on_file_received(d, header, path.to_string_lossy().into_owned())
                    }
                    PhoneEvent::FileProgress { id, sent, total } => listener.on_file_progress(d, id, sent, total),
                    PhoneEvent::FileSent { id } => listener.on_file_done(d, id, None),
                    PhoneEvent::FileFailed { id, reason } => listener.on_file_done(d, id, Some(reason)),
                }
            }
        });
        id
    }
}
