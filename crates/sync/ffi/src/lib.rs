//! El lado celular de `atic-sync`, para Kotlin.
//!
//! [`AticPhone`] tiene su propio runtime de tokio: Kotlin llama métodos
//! comunes (no suspend) y recibe todo por [`PhoneListener`], desde un hilo del
//! runtime. Quien implementa el listener pasa los datos a su hilo principal.

use std::sync::Arc;

use atic_sync::{
    phone::{LinkStatus, PairedDesktop, Phone, PhoneConfig, PhoneEvent, Target},
    AgentCard, ClipItem, Decision, MediaState, PcCommand, PcState,
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
    Connected { desktop_name: String },
    /// Se reintenta solo, salvo después de [`PhoneListener::on_rejected`].
    Offline { reason: String },
}

#[uniffi::export(with_foreign)]
pub trait PhoneListener: Send + Sync {
    /// Foto completa de los agentes del PC.
    fn on_agents(&self, agents: Vec<AgentCard>);
    fn on_link(&self, state: LinkState);
    /// El ticket funcionó. Guardar `desktop_json` y pasarlo a
    /// [`AticPhone::connect`] en los próximos arranques.
    fn on_paired(&self, desktop_json: String, desktop_name: String);
    /// El PC no acepta a este celular; no se reintenta.
    fn on_rejected(&self, reason: String);
    /// Algo que se copió en el PC.
    fn on_clip(&self, item: ClipItem);
    /// Lo que suena en el PC y si está grabando.
    fn on_pc(&self, pc: PcState);
    /// Historial del portapapeles del PC (vacío si está apagado).
    fn on_clip_history(&self, items: Vec<ClipItem>);
    /// La imagen pedida con `fetch_image`. Sin `data`: ya no está en el PC.
    fn on_image(&self, id: String, mime: String, data: Option<Vec<u8>>);
    /// La pill del PC pide play/pausa, siguiente o anterior.
    fn on_media_command(&self, command: PcCommand);
    /// El PC quiere estas imágenes del historial del celular (`send_clip_image`).
    fn on_clip_need(&self, ids: Vec<String>);
    /// Se borraron en el PC: borrarlas acá y recordarlo para los otros PCs.
    fn on_clip_deleted(&self, ids: Vec<String>);
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
    pub fn new(
        secret_key: Vec<u8>,
        device_name: String,
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
        let phone = runtime.block_on(Phone::start(PhoneConfig { secret_key, device_name }))?;
        Ok(Arc::new(Self { phone, listener, runtime }))
    }

    /// Parea con el texto del QR (`atic1:...`) y queda conectado.
    pub fn pair(&self, ticket: String) -> Result<(), SyncError> {
        let target = Target::from_ticket(&ticket)?;
        self.start(target);
        Ok(())
    }

    /// Se conecta al PC ya pareado (lo que llegó en `on_paired`).
    pub fn connect(&self, desktop_json: String) -> Result<(), SyncError> {
        let desktop = PairedDesktop::from_json(&desktop_json)?;
        self.start(Target::Known(desktop));
        Ok(())
    }

    /// `false` si no hay conexión: la decisión no salió.
    pub fn decide(&self, agent_id: String, permission_id: String, decision: Decision) -> bool {
        self.phone.decide(agent_id, permission_id, decision)
    }

    /// El historial guardado en el celular y lo borrado (al conectarse).
    pub fn sync_clips(&self, items: Vec<ClipItem>, deleted: Vec<String>) -> bool {
        self.phone.sync_clips(items, deleted)
    }

    pub fn send_clip_image(&self, id: String, mime: String, data: Vec<u8>) -> bool {
        self.phone.send_clip_image(id, mime, data)
    }

    pub fn delete_clips(&self, ids: Vec<String>) -> bool {
        self.phone.delete_clips(ids)
    }

    /// Lo que suena en el celular, para la pill del PC. `None`: nada.
    pub fn publish_media(&self, media: Option<MediaState>, art: Option<Vec<u8>>) -> bool {
        self.phone.publish_media(media, art)
    }

    /// Pide la imagen entera de un ítem del historial del PC.
    pub fn fetch_image(&self, id: String) -> bool {
        self.phone.fetch_image(id)
    }

    /// Manda una imagen (PNG o JPEG) al portapapeles del PC.
    pub fn send_image(&self, mime: String, data: Vec<u8>) -> bool {
        self.phone.send_image(mime, data)
    }

    /// Medios o detener la grabación. `false` si no hay conexión.
    pub fn command(&self, command: PcCommand) -> bool {
        self.phone.command(command)
    }

    /// Manda texto al portapapeles del PC. `false` si no hay conexión o es
    /// demasiado largo.
    pub fn send_clip(&self, text: String, copied_at_ms: i64) -> bool {
        self.phone.send_clip(text, copied_at_ms)
    }

    pub fn disconnect(&self) {
        self.phone.disconnect();
    }
}

impl AticPhone {
    fn start(&self, target: Target) {
        let _guard = self.runtime.enter();
        let (tx, mut rx) = unbounded_channel();
        self.phone.connect(target, tx);
        let listener = self.listener.clone();
        self.runtime.spawn(async move {
            while let Some(event) = rx.recv().await {
                match event {
                    PhoneEvent::Agents(agents) => listener.on_agents(agents),
                    PhoneEvent::Status(status) => listener.on_link(match status {
                        LinkStatus::Connecting => LinkState::Connecting,
                        LinkStatus::AwaitingApproval => LinkState::AwaitingApproval,
                        LinkStatus::Connected { desktop_name } => LinkState::Connected { desktop_name },
                        LinkStatus::Offline { reason } => LinkState::Offline { reason },
                    }),
                    PhoneEvent::Paired(desktop) => listener.on_paired(desktop.to_json(), desktop.name),
                    PhoneEvent::Rejected(reason) => listener.on_rejected(reason),
                    PhoneEvent::Clip(item) => listener.on_clip(item),
                    PhoneEvent::Pc(pc) => listener.on_pc(pc),
                    PhoneEvent::ClipHistory(items) => listener.on_clip_history(items),
                    PhoneEvent::Image { id, mime, data } => listener.on_image(id, mime, data),
                    PhoneEvent::MediaCommand(command) => listener.on_media_command(command),
                    PhoneEvent::ClipNeed(ids) => listener.on_clip_need(ids),
                    PhoneEvent::ClipDeleted(ids) => listener.on_clip_deleted(ids),
                }
            }
        });
    }
}
