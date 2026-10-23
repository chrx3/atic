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

    /// Medios o detener la grabación de ese PC. `false` si no está conectado.
    pub fn command(&self, desktop_id: String, command: PcCommand) -> bool {
        self.phone.command(&desktop_id, command)
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
                            LinkStatus::Connected { desktop_name } => LinkState::Connected { desktop_name },
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
                }
            }
        });
        id
    }
}
