//! El canal con la app del celular (`atic-sync`), como `phone_sync.rs` de la
//! app de Tauri: vincular, el portapapeles compartido en los dos sentidos, lo
//! que suena en el PC, detener una grabación desde el celular y los agentes
//! en curso, con sus permisos para contestarlos desde ahí.
//!
//! Usa los mismos archivos y la misma clave que Atic (`phone.json`,
//! `phone-deleted.json` y el llavero), así que los celulares ya vinculados
//! siguen vinculados. Apagado por defecto: el canal solo se abre si ya hay
//! celulares o si se pide un QR en Ajustes → Celular. Solo con `native_pill`
//! (si no, el canal es de la app de Tauri).

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use atic_core::{AppDirs, Config, MutexExt};
use atic_sync::desktop::{Desktop, DesktopConfig, DesktopEvent, PhoneRequest};
use atic_sync::{
    feature, ActivityKind, AgentAction, AgentActivity, AgentCard, AgentStatus, CallAction, ClipItem, ClipKind, Decision, FileHeader,
    FileKind, MediaState, PcCommand, PcState, PermissionAsk, RecordingState,
};
use serde::{Deserialize, Serialize};

use crate::media::{Control, Media};

/// Lo que el celular le pide a la ventana de la pill.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    StopRecording,
    /// Texto para el agente de esa sesión que no tiene consola: si vive en una
    /// conversación de Atic Code, va a ese chat, con sus imágenes (`mime` y bytes).
    Prompt { session: String, text: String, images: Vec<(String, Vec<u8>)> },
    /// Aceptar, abrir o detener el agente de esa sesión (`AgentAction`).
    AgentAction { session: String, action: AgentAction },
    /// Un enlace que el celular mandó a abrir aquí (ya revisado: `http` o `https`).
    OpenUrl(String),
}

/// Lo nuevo que la pill atiende (`feature`): archivos, mirar un agente en
/// vivo y los avisos y llamadas del celular (en la bandeja del notch,
/// `tray.rs`), hacer sonar el celular, abrir enlaces en el otro y pedirle una
/// foto. Escribirle a un agente y controlar
/// el PC (también el panel táctil y el teclado) teclean en este PC, así que
/// van solo si se prendieron en Ajustes → Celular. El celular solo manda lo
/// prendido.
fn enabled_features(devices: &Devices) -> Vec<String> {
    let mut features = vec![
        feature::FILES,
        feature::NOTICES,
        feature::CALLS,
        feature::WATCH,
        feature::FIND,
        feature::LINKS,
        feature::PHOTO,
        feature::ACCESS,
        feature::AGENTS,
        // La pantalla del celular en una ventana del PC (`phone_screen.rs`): la
        // comparte el celular, a pedido de quien lo tiene en la mano.
        feature::SCREEN,
    ];
    if devices.prompt {
        features.push(feature::PROMPT);
    }
    if devices.control {
        features.push(feature::CONTROL);
        features.push(feature::REMOTE);
        // Ver la pantalla del PC en el celular (y tocarla) es tan sensible como controlarlo.
        features.push(feature::SCREEN_OUT);
    }
    features.into_iter().map(str::to_string).collect()
}

/// Un aviso o la llamada del celular, como los muestra la bandeja del notch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Alert {
    Notice {
        device: String,
        key: String,
        app: String,
        title: String,
        text: String,
        at_ms: i64,
        /// Se puede contestar desde aquí (WhatsApp, Telegram, Mensajes…).
        can_reply: bool,
        /// La foto del contacto o el ícono de la app (PNG de 64 px).
        icon: Option<Arc<gpui::Image>>,
    },
    /// `since_ms`: desde cuándo se habla; `None`, sonando.
    Call { device: String, name: String, incoming: bool, since_ms: Option<i64>, can_answer: bool, can_hang_up: bool },
    /// El celular `name` pide prender funciones que teclean en este PC
    /// (`feature::requestable`). Queda hasta que se conteste (`access_answer`).
    Access { device: String, name: String, features: Vec<String> },
}

/// Lo que el celular tiene a la vista ahora: sus avisos y su llamada.
static ALERTS: Mutex<Vec<Alert>> = Mutex::new(Vec::new());
/// Cambió algo en `ALERTS`: el notch lo pone al día en su próximo tick, sin
/// esperar el sondeo de cada segundo de la bandeja.
static ALERTS_CHANGED: AtomicBool = AtomicBool::new(false);

/// Los avisos y la llamada de los celulares, para la bandeja del notch.
pub fn alerts() -> Vec<Alert> {
    ALERTS.lock_or_recover().clone()
}

/// Si cambió algo desde la última vez que se preguntó.
pub fn take_alerts_changed() -> bool {
    ALERTS_CHANGED.swap(false, Ordering::Relaxed)
}

fn alerts_changed() {
    ALERTS_CHANGED.store(true, Ordering::Relaxed);
}

/// Contestar un aviso desde el PC (la respuesta rápida de la notificación).
/// `false` si el celular ya no está o no recibe respuestas.
pub fn notice_reply(device: &str, key: &str, text: &str) -> bool {
    RUNNING.lock_or_recover().as_ref().is_some_and(|running| running.desktop.notice_reply(device, key.to_string(), text.to_string()))
}

/// «Descartar» un aviso: se va también del celular.
pub fn notice_dismiss(device: &str, key: &str) {
    ALERTS.lock_or_recover().retain(|alert| !matches!(alert, Alert::Notice { device: d, key: k, .. } if d == device && k == key));
    alerts_changed();
    if let Some(running) = RUNNING.lock_or_recover().as_ref() {
        running.desktop.notice_dismiss(device, key.to_string());
    }
}

/// «Permitir» o «Negar» lo que pidió el celular. Permitir lo prende como el
/// interruptor de Ajustes → Celular (para todos los celulares) y el celular lo
/// ve al tiro, sin reconectar.
pub fn access_answer(device: &str, allow: bool) {
    let features = {
        let mut alerts = ALERTS.lock_or_recover();
        let features = alerts.iter().find_map(|alert| match alert {
            Alert::Access { device: d, features, .. } if d == device => Some(features.clone()),
            _ => None,
        });
        alerts.retain(|alert| !matches!(alert, Alert::Access { device: d, .. } if d == device));
        features
    };
    alerts_changed();
    let Some(features) = features else {
        return;
    };
    tracing::info!(permitido = allow, ?features, "celular: pedido de permiso contestado");
    // Si no se pudo guardar, no quedó prendido: el celular oye que no.
    let allow = allow && {
        let devices = load_devices();
        let prompt = devices.prompt || features.iter().any(|f| f == feature::PROMPT);
        let control = devices.control || features.iter().any(|f| f == feature::CONTROL || f == feature::REMOTE);
        set_allowed(prompt, control).map_err(|error| tracing::warn!(%error, "celular: no se pudo guardar el permiso")).is_ok()
    };
    if let Some(running) = RUNNING.lock_or_recover().as_ref() {
        running.desktop.answer_access(device, features, allow);
    }
}

/// Contestar o colgar la llamada del celular.
pub fn call_action(device: &str, answer: bool) {
    if let Some(running) = RUNNING.lock_or_recover().as_ref() {
        running.desktop.call_action(device, if answer { CallAction::Answer } else { CallAction::HangUp });
    }
}

const TICK: Duration = Duration::from_millis(250);
/// Avisos del celular que guarda la bandeja a la vez.
const MAX_NOTICES: usize = 50;
const PC_REFRESH: Duration = Duration::from_secs(2);
const TOMBSTONE_DAYS: i64 = 60;
const THUMB_PX: u32 = 200;
const FULL_MAX_PX: u32 = 2560;

static RUNTIME: OnceLock<Option<tokio::runtime::Runtime>> = OnceLock::new();
static RUNNING: Mutex<Option<Running>> = Mutex::new(None);
/// Evita dos arranques a la vez (el del inicio y el de un QR pedido enseguida).
static STARTING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
static CLIPBOARD_ON: AtomicBool = AtomicBool::new(false);
/// Ya se guardó la copia de un `phone.json` que no se pudo leer.
static BAD_SAVED: AtomicBool = AtomicBool::new(false);
static PC_DIRTY: AtomicBool = AtomicBool::new(false);
/// Lo último que llegó del celular al portapapeles: el watcher lo va a ver
/// como copia nueva y no hay que devolvérselo.
static FROM_PHONE: Mutex<Option<String>> = Mutex::new(None);
static PENDING_IMPORTS: Mutex<Option<HashMap<String, (u64, bool)>>> = Mutex::new(None);
static THUMBS: Mutex<Option<HashMap<String, Option<(Vec<u8>, u32, u32)>>>> = Mutex::new(None);
/// Desde cuándo graba la pill (ms), si graba. Lo fija la ventana.
static RECORDING: Mutex<Option<i64>> = Mutex::new(None);
static MEDIA: OnceLock<Media> = OnceLock::new();
static COMMANDS: OnceLock<Sender<Command>> = OnceLock::new();
/// El celular cuya música se muestra: a él van los controles.
static PHONE_MEDIA_DEVICE: Mutex<Option<String>> = Mutex::new(None);
/// Los agentes tal como los ve el notch de Agentes; se publican si cambian.
static AGENTS: Mutex<Option<Vec<AgentCard>>> = Mutex::new(None);

struct Running {
    desktop: Arc<Desktop>,
    connected: HashSet<String>,
    pending: Option<(String, String)>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Devices {
    devices: Vec<Device>,
    #[serde(default)]
    clipboard: bool,
    /// El celular puede escribirle a los agentes (lo teclea en su consola).
    #[serde(default)]
    prompt: bool,
    /// El celular controla el PC: volumen, bloquear, diapositivas.
    #[serde(default)]
    control: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Device {
    id: String,
    name: String,
    paired_at: i64,
}

/// Lo que muestra Ajustes → Celular.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Status {
    pub running: bool,
    pub clipboard: bool,
    pub prompt: bool,
    pub control: bool,
    /// Id, nombre y si está conectado.
    pub devices: Vec<(String, String, bool)>,
    /// Un celular que escaneó el QR y espera que lo acepten: id y nombre.
    pub pending: Option<(String, String)>,
}

fn runtime() -> Option<&'static tokio::runtime::Runtime> {
    RUNTIME
        .get_or_init(|| {
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .thread_name("celular")
                .enable_all()
                .build()
                .map_err(|error| tracing::warn!(%error, "celular: no arrancó el runtime"))
                .ok()
        })
        .as_ref()
}

fn data_file(name: &str) -> Option<PathBuf> {
    Some(AppDirs::new().ok()?.config_path().with_file_name(name))
}

fn load_devices() -> Devices {
    let path = data_file("phone.json");
    let raw = path.as_ref().and_then(|path| std::fs::read_to_string(path).ok());
    let devices: Devices = match raw.as_deref().map(serde_json::from_str::<Devices>) {
        Some(Ok(devices)) => devices,
        // Un archivo que no se entiende no se pisa en silencio: se rescata lo que
        // se pueda (los celulares vinculados, uno por uno) para que el próximo
        // guardado no obligue a vincularlos de nuevo, y queda una copia (una
        // vez por sesión: no se pisa con cada lectura).
        Some(Err(error)) => {
            let devices = raw.as_deref().map(salvage_devices).unwrap_or_default();
            if !BAD_SAVED.swap(true, Ordering::Relaxed) {
                tracing::error!(%error, rescatados = devices.devices.len(), "celular: phone.json no se pudo leer; queda una copia en phone.json.bad");
                if let Some(path) = &path {
                    let _ = std::fs::copy(path, path.with_extension("json.bad"));
                }
            }
            devices
        }
        None => Devices::default(),
    };
    CLIPBOARD_ON.store(devices.clipboard, Ordering::Relaxed);
    devices
}

/// Lo que se entiende de un `phone.json` dañado: cada celular que se lea
/// entero y los interruptores que sean `true`; lo demás, como recién instalado.
fn salvage_devices(raw: &str) -> Devices {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(raw) else {
        return Devices::default();
    };
    let flag = |key: &str| value.get(key).and_then(serde_json::Value::as_bool).unwrap_or(false);
    Devices {
        devices: value
            .get("devices")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|device| serde_json::from_value(device.clone()).ok())
            .collect(),
        clipboard: flag("clipboard"),
        prompt: flag("prompt"),
        control: flag("control"),
    }
}

fn save_devices(devices: &Devices) -> Result<(), String> {
    let path = data_file("phone.json").ok_or("sin carpeta de datos")?;
    let json = serde_json::to_string_pretty(devices).map_err(|e| e.to_string())?;
    atic_core::write_atomic_str(&path, &json).map_err(|e| e.to_string())
}

/// Al arrancar la pill: guarda con qué controlar la música y por dónde mandar
/// comandos, y abre el canal si ya hay celulares (y la pill es la nativa).
pub fn start(media: Media) -> Receiver<Command> {
    let _ = MEDIA.set(media);
    let (tx, rx) = std::sync::mpsc::channel();
    let _ = COMMANDS.set(tx);
    // La pill de desarrollo no sincroniza: lo hace la de siempre.
    let native = !crate::dev::on() && AppDirs::new().is_ok_and(|dirs| Config::load(&dirs.config_path()).native_pill);
    if native && !load_devices().devices.is_empty() {
        if let Some(rt) = runtime() {
            rt.spawn(async {
                if let Err(error) = ensure_running().await {
                    tracing::warn!(%error, "celular: no se pudo abrir el canal");
                }
            });
        }
    }
    rx
}

/// La pill empezó o terminó de grabar (`started_ms` desde 1970).
/// Al salir: corta el canal avisando, para que el celular muestre «sin
/// conexión» ya y no recién cuando la conexión vence sola.
pub fn stop() {
    let Some(desktop) = RUNNING.lock_or_recover().take().map(|r| r.desktop) else { return };
    if let Some(rt) = runtime() {
        // El `timeout` (y soltar el canal) adentro del runtime: armado afuera, entra en pánico.
        rt.block_on(async move {
            let _ = tokio::time::timeout(std::time::Duration::from_millis(300), desktop.close()).await;
        });
    }
}

pub fn set_recording(started_ms: Option<i64>) {
    let mut slot = RECORDING.lock_or_recover();
    // Se avisa en cada cuadro y la cuenta varía en milisegundos: mientras siga
    // grabando, vale el primer inicio.
    if slot.is_some() && started_ms.is_some() {
        return;
    }
    if *slot != started_ms {
        *slot = started_ms;
        PC_DIRTY.store(true, Ordering::Relaxed);
    }
}

pub fn status() -> Status {
    let devices = load_devices();
    let guard = RUNNING.lock_or_recover();
    let connected = guard.as_ref().map(|r| r.connected.clone()).unwrap_or_default();
    Status {
        running: guard.is_some(),
        clipboard: devices.clipboard,
        prompt: devices.prompt,
        control: devices.control,
        pending: guard.as_ref().and_then(|r| r.pending.clone()),
        devices: devices
            .devices
            .into_iter()
            .map(|d| {
                let on = connected.contains(&d.id);
                (d.id, d.name, on)
            })
            .collect(),
    }
}

/// Abre el canal si hace falta y devuelve el ticket del QR (vale 5 minutos).
/// Bloquea: llamar fuera del hilo de la ventana.
pub fn pair_start() -> Result<String, String> {
    let rt = runtime().ok_or("sin runtime")?;
    rt.block_on(async {
        let desktop = ensure_running().await?;
        Ok(desktop.pairing_ticket().await)
    })
}

pub fn pair_cancel() {
    if let Some(running) = RUNNING.lock_or_recover().as_ref() {
        running.desktop.cancel_pairing();
    }
}

/// Acepta o rechaza al celular que escaneó el QR.
pub fn pair_answer(device_id: &str, accept: bool) -> Result<(), String> {
    let desktop = {
        let mut guard = RUNNING.lock_or_recover();
        let running = guard.as_mut().ok_or("el canal con el celular no está abierto")?;
        running.pending = None;
        running.desktop.clone()
    };
    if desktop.approve_pairing(device_id, accept) {
        Ok(())
    } else {
        Err(crate::i18n::t("pill.phone.expired").into())
    }
}

pub fn set_clipboard(enabled: bool) -> Result<(), String> {
    let mut devices = load_devices();
    devices.clipboard = enabled;
    save_devices(&devices)?;
    CLIPBOARD_ON.store(enabled, Ordering::Relaxed);
    PC_DIRTY.store(true, Ordering::Relaxed);
    Ok(())
}

/// Prende o apaga escribirle a los agentes y controlar el PC desde el
/// celular. Vale al tiro: lo que llegue después ya no pasa.
pub fn set_allowed(prompt: bool, control: bool) -> Result<(), String> {
    let mut devices = load_devices();
    devices.prompt = prompt;
    devices.control = control;
    save_devices(&devices)?;
    if let Some(running) = RUNNING.lock_or_recover().as_ref() {
        running.desktop.enable(enabled_features(&devices), data_file("phone-inbox"));
    }
    Ok(())
}

/// Si hay un celular conectado que recibe archivos.
pub fn can_send_files() -> bool {
    RUNNING.lock_or_recover().as_ref().is_some_and(|running| !running.connected.is_empty())
}

/// Manda `path` a los celulares conectados (llega a Descargas/Atic, con un
/// aviso para abrirlo). `false` si no hay a quién; el resultado se avisa solo.
pub fn send_file(path: PathBuf) -> bool {
    let Some((desktop, devices)) = RUNNING.lock_or_recover().as_ref().map(|running| (running.desktop.clone(), running.connected.iter().cloned().collect::<Vec<_>>())) else {
        return false;
    };
    let Some(rt) = runtime().filter(|_| !devices.is_empty()) else {
        return false;
    };
    rt.spawn(async move {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        for device in devices {
            let result = desktop.send_file(&device, &path, mime_of(&path).to_string(), FileKind::File, None).await;
            let (title, body) = match &result {
                Ok(()) => (crate::i18n::t("pill.phone.sentToPhone"), name.clone()),
                Err(error) => {
                    tracing::warn!(%error, "celular: no se pudo mandar el archivo");
                    (crate::i18n::t("pill.phone.sendFailed"), format!("{name}: {error}"))
                }
            };
            #[cfg(any(windows, target_os = "macos"))]
            crate::platform::tray_icon::notify(title, &body, false, None);
            #[cfg(not(any(windows, target_os = "macos")))]
            let _ = (title, body);
        }
    });
    true
}

/// El tipo del archivo por su extensión, para que el celular sepa con qué abrirlo.
fn mime_of(path: &std::path::Path) -> &'static str {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "heic" => "image/heic",
        "pdf" => "application/pdf",
        "txt" | "md" => "text/plain",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "m4a" => "audio/mp4",
        "mp4" => "video/mp4",
        "mov" => "video/quicktime",
        "zip" => "application/zip",
        _ => "application/octet-stream",
    }
}

pub fn unpair(device_id: &str) -> Result<(), String> {
    let mut devices = load_devices();
    devices.devices.retain(|d| d.id != device_id);
    save_devices(&devices)?;
    if let Some(running) = RUNNING.lock_or_recover().as_ref() {
        running.desktop.revoke(device_id).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Lo llama el watcher del historial con cada texto nuevo (ya sin lo sensible).
pub fn clipboard_copied(text: &str) {
    {
        let mut from_phone = FROM_PHONE.lock_or_recover();
        if from_phone.as_deref() == Some(text) {
            *from_phone = None;
            return;
        }
    }
    if !CLIPBOARD_ON.load(Ordering::Relaxed) {
        return;
    }
    if let Some(desktop) = RUNNING.lock_or_recover().as_ref().map(|r| r.desktop.clone()) {
        desktop.send_clip(text.to_string(), chrono::Utc::now().timestamp_millis());
    }
}

async fn ensure_running() -> Result<Arc<Desktop>, String> {
    // Usa la misma clave que la de siempre: dos canales con la misma
    // identidad se pelean al celular.
    if crate::dev::on() {
        return Err("La pill de desarrollo no abre el canal del celular: vincúlalo desde la de siempre.".into());
    }
    let _starting = STARTING.lock().await;
    if let Some(running) = RUNNING.lock_or_recover().as_ref() {
        return Ok(running.desktop.clone());
    }
    let devices = load_devices();
    // El saludo lleva lo prendido. Lo que llega del celular espera en
    // `phone-inbox` hasta que se ordena.
    let inbox = data_file("phone-inbox");
    if let Some(inbox) = &inbox {
        let _ = std::fs::create_dir_all(inbox);
    }
    let (desktop, events) = Desktop::start(DesktopConfig {
        secret_key: load_or_create_key()?,
        name: desktop_name(),
        trusted: devices.devices.iter().map(|d| d.id.clone()).collect(),
        features: enabled_features(&devices),
        inbox,
    })
    .await
    .map_err(|e| format!("{e:#}"))?;
    desktop.on_video(crate::phone_screen::accept);
    desktop.on_screen_request(crate::phone_screen::share);
    let desktop = Arc::new(desktop);
    *RUNNING.lock_or_recover() = Some(Running { desktop: desktop.clone(), connected: HashSet::new(), pending: None });
    if let Some(rt) = runtime() {
        rt.spawn(handle_events(events));
        rt.spawn(publish_loop(desktop.clone()));
    }
    Ok(desktop)
}

async fn handle_events(mut events: tokio::sync::mpsc::UnboundedReceiver<DesktopEvent>) {
    while let Some(event) = events.recv().await {
        match event {
            DesktopEvent::PairRequested { device_id, device_name } => {
                if let Some(running) = RUNNING.lock_or_recover().as_mut() {
                    running.pending = Some((device_id, device_name));
                }
            }
            DesktopEvent::Paired { device_id, device_name } => {
                let mut devices = load_devices();
                devices.devices.retain(|d| d.id != device_id);
                devices.devices.push(Device { id: device_id, name: device_name, paired_at: chrono::Utc::now().timestamp_millis() });
                if let Err(error) = save_devices(&devices) {
                    tracing::warn!(%error, "celular: no se pudo guardar el vinculado");
                }
                if let Some(running) = RUNNING.lock_or_recover().as_mut() {
                    running.pending = None;
                }
            }
            DesktopEvent::Connected { device_id, device_name, features } => {
                tracing::info!(equipo = %device_id, nombre = %device_name, sabe = %features.join(","), "celular conectado");
                if let Some(running) = RUNNING.lock_or_recover().as_mut() {
                    running.connected.insert(device_id);
                }
            }
            DesktopEvent::Disconnected { device_id } => {
                tracing::info!(equipo = %device_id, "celular desconectado");
                if let Some(running) = RUNNING.lock_or_recover().as_mut() {
                    running.connected.remove(&device_id);
                }
                // Sin el celular, sus avisos y su llamada ya no se pueden atender.
                ALERTS.lock_or_recover().retain(|alert| match alert {
                    Alert::Notice { device, .. } | Alert::Call { device, .. } | Alert::Access { device, .. } => *device != device_id,
                });
                alerts_changed();
                stop_watch(&device_id);
                // Lo que sonaba en ese celular ya no se puede controlar.
                if PHONE_MEDIA_DEVICE.lock_or_recover().as_deref() == Some(device_id.as_str()) {
                    show_phone_media(device_id, None, None);
                }
            }
            DesktopEvent::Clip { item, .. } if CLIPBOARD_ON.load(Ordering::Relaxed) => {
                let text = item.text.trim().to_string();
                *FROM_PHONE.lock_or_recover() = Some(text.clone());
                if let Err(error) = atic_clipboard::set_system_text(text) {
                    tracing::warn!(%error, "celular: no se pudo pegar lo que mandó");
                }
            }
            DesktopEvent::ClipSync { device_id, items, deleted } if CLIPBOARD_ON.load(Ordering::Relaxed) => {
                merge_from_phone(&device_id, items, deleted);
            }
            DesktopEvent::ClipImage { id, data, .. } => {
                let meta = PENDING_IMPORTS.lock_or_recover().get_or_insert_with(HashMap::new).remove(&id);
                if let (Some((created_at, pinned)), true) = (meta, CLIPBOARD_ON.load(Ordering::Relaxed)) {
                    let result = tokio::task::spawn_blocking(move || {
                        crate::clipboard_owner::with_history(|history, dir| {
                            history.import_image(dir, &id, &data, created_at, pinned, crate::clipboard_owner::image_label)
                        })
                    })
                    .await;
                    if let Ok(Some(Err(error))) = result {
                        tracing::warn!(%error, "celular: no se pudo sumar una imagen");
                    }
                    PC_DIRTY.store(true, Ordering::Relaxed);
                }
            }
            DesktopEvent::ClipDelete { ids, .. } => {
                remember_deleted(&ids);
                crate::clipboard_owner::with_history(|history, dir| history.delete_ids(dir, &ids));
                PC_DIRTY.store(true, Ordering::Relaxed);
            }
            DesktopEvent::FetchImage { device_id, id } => {
                let desktop = RUNNING.lock_or_recover().as_ref().map(|r| r.desktop.clone());
                if let Some(desktop) = desktop {
                    let found = tokio::task::spawn_blocking({
                        let id = id.clone();
                        move || if CLIPBOARD_ON.load(Ordering::Relaxed) { full_image(&id) } else { None }
                    })
                    .await
                    .ok()
                    .flatten();
                    let (mime, data) = match found {
                        Some((mime, data)) => (mime, Some(data)),
                        None => ("image/png".to_string(), None),
                    };
                    desktop.send_image(&device_id, id, mime, data);
                }
            }
            DesktopEvent::Image { data, .. } if CLIPBOARD_ON.load(Ordering::Relaxed) => {
                let pasted = tokio::task::spawn_blocking(move || paste_image(&data)).await;
                if let Ok(Err(error)) = pasted {
                    tracing::warn!(%error, "celular: no se pudo pegar la imagen");
                }
            }
            DesktopEvent::Command { command, .. } => {
                run_command(command);
                PC_DIRTY.store(true, Ordering::Relaxed);
            }
            DesktopEvent::Decide { agent_id, permission_id, decision, .. } => {
                let allow = decision != Decision::Deny;
                let decided =
                    tokio::task::spawn_blocking(move || crate::agent_prompts::decide(&agent_id, &permission_id, allow))
                        .await;
                if let Ok(Err(error)) = decided {
                    tracing::warn!(%error, "celular: no se pudo contestar el permiso");
                }
            }
            DesktopEvent::PhoneMedia { device_id, media, art } => show_phone_media(device_id, media, art),
            DesktopEvent::Phone { device_id, request } => handle_request(device_id, request).await,
            // Las preguntas con opciones se contestan en la consola.
            _ => {}
        }
    }
}

/// Lo nuevo que pide el celular (solo llega lo de `enabled_features`).
async fn handle_request(device: String, request: PhoneRequest) {
    match request {
        PhoneRequest::Notice(notice) => {
            let alert = Alert::Notice {
                device: device.clone(),
                key: notice.key.clone(),
                app: notice.app_name,
                title: notice.title,
                text: notice.text,
                at_ms: notice.posted_at_ms,
                can_reply: notice.can_reply,
                icon: notice.icon.map(|png| Arc::new(gpui::Image::from_bytes(gpui::ImageFormat::Png, png))),
            };
            let mut alerts = ALERTS.lock_or_recover();
            alerts.retain(|old| !matches!(old, Alert::Notice { device: d, key, .. } if *d == device && *key == notice.key));
            alerts.push(alert);
            // Una ráfaga de avisos no crece sin fin: quedan los más nuevos.
            let notices = alerts.iter().filter(|a| matches!(a, Alert::Notice { .. })).count();
            if notices > MAX_NOTICES {
                let mut extra = notices - MAX_NOTICES;
                alerts.retain(|a| {
                    if extra > 0 && matches!(a, Alert::Notice { .. }) {
                        extra -= 1;
                        return false;
                    }
                    true
                });
            }
            alerts_changed();
        }
        PhoneRequest::NoticeRemoved { key } => {
            ALERTS.lock_or_recover().retain(|alert| !matches!(alert, Alert::Notice { device: d, key: k, .. } if *d == device && *k == key));
            alerts_changed();
        }
        PhoneRequest::Call(state) => {
            // Sin el nombre: solo el paso (sonando, en curso, terminó).
            tracing::info!(
                sonando = state.as_ref().map(|s| s.incoming),
                en_curso = state.as_ref().map(|s| s.since_ms.is_some()),
                "celular: llamada"
            );
            let mut alerts = ALERTS.lock_or_recover();
            alerts.retain(|alert| !matches!(alert, Alert::Call { device: d, .. } if *d == device));
            if let Some(state) = state {
                alerts.push(Alert::Call {
                    device,
                    name: state.name,
                    incoming: state.incoming,
                    since_ms: state.since_ms,
                    can_answer: state.can_answer,
                    can_hang_up: state.can_hang_up,
                });
            }
            alerts_changed();
        }
        PhoneRequest::Prompt { agent_id, text, images, clip_ids } => {
            let mut images: Vec<(String, Vec<u8>)> = images.into_iter().map(|i| (i.mime, i.data)).collect();
            // Las del historial del PC se leen aquí, si se comparte el portapapeles.
            if !clip_ids.is_empty() && CLIPBOARD_ON.load(Ordering::Relaxed) {
                let found = tokio::task::spawn_blocking(move || clip_ids.iter().filter_map(|id| full_image(id)).collect::<Vec<_>>()).await;
                images.extend(found.unwrap_or_default());
            }
            // Con imágenes, primero una conversación de Atic Code: la consola no
            // las recibe (si no la hay, la ventana teclea el texto en la consola).
            if !images.is_empty() {
                tracing::info!(imagenes = images.len(), "celular: mensaje con imágenes para un agente");
                if let Some(tx) = COMMANDS.get() {
                    let _ = tx.send(Command::Prompt { session: agent_id, text, images });
                }
                return;
            }
            // Las imágenes no se pudieron leer y no queda texto: nada que mandar.
            if text.trim().is_empty() {
                return;
            }
            let typed = tokio::task::spawn_blocking({
                let (session, text) = (agent_id.clone(), text.clone());
                move || crate::agent_prompts::type_text(&session, &text)
            })
            .await;
            // Sin consola puede ser una conversación de Atic Code: la resuelve la ventana.
            if !matches!(typed, Ok(Ok(()))) {
                if let Some(tx) = COMMANDS.get() {
                    let _ = tx.send(Command::Prompt { session: agent_id, text, images: Vec::new() });
                }
            }
        }
        PhoneRequest::AgentAction { agent_id, action } => {
            if let Some(tx) = COMMANDS.get() {
                let _ = tx.send(Command::AgentAction { session: agent_id, action });
            }
        }
        PhoneRequest::File { header, path } => {
            let _ = tokio::task::spawn_blocking(move || receive_file(header, path)).await;
        }
        PhoneRequest::Input(event) => crate::platform::control::input(event),
        PhoneRequest::Access { features } => {
            let devices = load_devices();
            let name = devices.devices.iter().find(|d| d.id == device).map(|d| d.name.clone()).unwrap_or_default();
            // Lo ya prendido no se vuelve a preguntar.
            let missing: Vec<String> = features
                .into_iter()
                .filter(|f| match f.as_str() {
                    feature::PROMPT => !devices.prompt,
                    feature::CONTROL | feature::REMOTE => !devices.control,
                    _ => false,
                })
                .collect();
            tracing::info!(equipo = %device, ?missing, "celular: pide permiso");
            if missing.is_empty() {
                // Ya estaba: se contesta que sí y el celular se pone al día.
                if let Some(running) = RUNNING.lock_or_recover().as_ref() {
                    running.desktop.enable(enabled_features(&devices), data_file("phone-inbox"));
                    running.desktop.answer_access(&device, Vec::new(), true);
                }
                return;
            }
            let mut alerts = ALERTS.lock_or_recover();
            alerts.retain(|alert| !matches!(alert, Alert::Access { device: d, .. } if *d == device));
            alerts.push(Alert::Access { device, name, features: missing });
            alerts_changed();
        }
        PhoneRequest::OpenUrl { url } => {
            if let Some(tx) = COMMANDS.get() {
                let _ = tx.send(Command::OpenUrl(url));
            }
        }
        // La foto que se pidió con «Foto» (`take_photo`): al portapapeles aunque
        // el compartido esté apagado (se pidió desde aquí), y el historial la
        // recoge. Una que nadie pidió no pisa lo copiado.
        PhoneRequest::Photo { data, .. } => {
            if !photo_was_asked(&device) {
                tracing::warn!(equipo = %device, "celular: llegó una foto que nadie pidió; no se pega");
                return;
            }
            let pasted = tokio::task::spawn_blocking(move || paste_image(&data)).await;
            let (title, body) = match pasted {
                Ok(Ok(())) => (crate::i18n::t("pill.phone.photoArrived"), crate::i18n::t("pill.phone.photoCopied").to_string()),
                Ok(Err(error)) => {
                    tracing::warn!(%error, "celular: no se pudo pegar la foto");
                    (crate::i18n::t("pill.phone.photoFailed"), error)
                }
                Err(_) => return,
            };
            #[cfg(any(windows, target_os = "macos"))]
            crate::platform::tray_icon::notify(title, &body, false, None);
            #[cfg(not(any(windows, target_os = "macos")))]
            let _ = (title, body);
        }
        PhoneRequest::Watch { agent_id } => {
            stop_watch(&device);
            if let Some(agent_id) = agent_id {
                let task = tokio::spawn(watch_agent(device.clone(), agent_id));
                WATCHES.lock_or_recover().insert(device, task.abort_handle());
            }
        }
    }
}

/// Lo que mira cada celular en vivo: su tarea, para cortarla.
static WATCHES: std::sync::LazyLock<Mutex<HashMap<String, tokio::task::AbortHandle>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));
/// Al empezar a mirar, cuántas líneas hacia atrás.
const WATCH_LINES: usize = 200;
/// Cada cuánto se mira si el agente escribió algo.
const WATCH_EVERY: Duration = Duration::from_millis(700);

fn stop_watch(device: &str) {
    if let Some(task) = WATCHES.lock_or_recover().remove(device) {
        task.abort();
    }
}

fn agent_output(device: &str, agent_id: &str, lines: Vec<String>, reset: bool) -> bool {
    RUNNING.lock_or_recover().as_ref().is_some_and(|running| running.desktop.agent_output(device, agent_id.to_string(), lines, reset))
}

/// Manda al celular las últimas líneas de la sesión `agent_id` y después lo
/// que va escribiendo, hasta que deja de mirar o se desconecta.
async fn watch_agent(device: String, agent_id: String) {
    let id = agent_id.clone();
    let Ok(Some(mut transcript)) = tokio::task::spawn_blocking(move || crate::agents::Transcript::open(&id)).await else {
        agent_output(&device, &agent_id, vec![crate::i18n::t("pill.phone.agentNotFound").into()], true);
        return;
    };
    // Un celular que dibuja la conversación como chat la recibe por partes,
    // con Markdown e imágenes; uno viejo, en líneas.
    let rich = RUNNING.lock_or_recover().as_ref().is_some_and(|r| r.desktop.phone_features(&device).iter().any(|f| f == feature::AGENTS));
    if rich {
        return watch_entries(device, agent_id, transcript).await;
    }
    let mut lines = transcript.read();
    let skip = lines.len().saturating_sub(WATCH_LINES);
    lines.drain(..skip);
    if !agent_output(&device, &agent_id, lines, true) {
        return;
    }
    loop {
        tokio::time::sleep(WATCH_EVERY).await;
        let lines = transcript.read();
        if !lines.is_empty() && !agent_output(&device, &agent_id, lines, false) {
            return;
        }
    }
}

/// Al empezar a mirar por partes, cuántas hacia atrás.
const WATCH_ENTRIES: usize = 60;

/// `watch_agent` por partes (`AgentEntries`). Leer achica imágenes: va aparte.
async fn watch_entries(device: String, agent_id: String, transcript: crate::agents::Transcript) {
    let mut transcript = Some(transcript);
    let mut first = true;
    loop {
        if !first {
            tokio::time::sleep(WATCH_EVERY).await;
        }
        let Some(mut current) = transcript.take() else {
            return;
        };
        let Ok((back, entries)) = tokio::task::spawn_blocking(move || {
            // Al empezar, solo las últimas: las imágenes de las demás no se achican.
            let entries = current.read_entries(first.then_some(WATCH_ENTRIES));
            (current, entries)
        })
        .await
        else {
            return;
        };
        transcript = Some(back);
        if !first && entries.is_empty() {
            continue;
        }
        let sent = RUNNING
            .lock_or_recover()
            .as_ref()
            .is_some_and(|running| running.desktop.agent_entries(&device, agent_id.clone(), entries, first));
        if !sent {
            return;
        }
        first = false;
    }
}

/// Lo último que mandó el celular con Compartir (archivos sueltos, no
/// reuniones), lo más nuevo primero: «Recibido del celular» en el notch.
static RECEIVED: Mutex<Option<Vec<PathBuf>>> = Mutex::new(None);
const MAX_RECEIVED: usize = 8;

/// Lo recibido que sigue en su lugar (lo movido o borrado sale de la lista).
pub fn received() -> Vec<PathBuf> {
    let mut slot = RECEIVED.lock_or_recover();
    let list = slot.get_or_insert_with(|| {
        data_file("phone-received.json")
            .and_then(|path| std::fs::read_to_string(path).ok())
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default()
    });
    list.retain(|path| path.exists());
    list.clone()
}

fn remember_received(path: &std::path::Path) {
    let mut list = received();
    list.retain(|p| p != path);
    list.insert(0, path.to_path_buf());
    list.truncate(MAX_RECEIVED);
    if let Some(file) = data_file("phone-received.json") {
        let _ = std::fs::write(file, serde_json::to_string(&list).unwrap_or_default());
    }
    *RECEIVED.lock_or_recover() = Some(list);
    alerts_changed();
}

/// Una foto o captura que el portapapeles sabe pegar (PNG o JPEG).
pub fn is_image(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "png" | "jpg" | "jpeg"))
}

/// Copia al portapapeles un archivo recibido: la imagen misma si es una foto;
/// si no, el archivo (para pegarlo en el Finder o en un chat).
pub fn copy_received(path: &std::path::Path) -> Result<(), String> {
    if is_image(path) {
        return std::fs::read(path).map_err(|e| e.to_string()).and_then(|data| paste_image(&data));
    }
    crate::clip_image::write_files(&[path.to_string_lossy().into_owned()])
}

/// Lo que mandó el celular, de `phone-inbox` a Descargas/Atic, con un aviso.
/// Una foto queda además en el portapapeles, lista para pegar.
/// Una reunión (el audio y su transcripción llegan aparte, con el mismo `id`)
/// va junta a Descargas/Atic/Reuniones/<fecha> <título>.
fn receive_file(header: FileHeader, path: PathBuf) {
    let Some(downloads) = crate::paths::home().map(|home| home.join("Downloads").join("Atic")) else {
        return;
    };
    let (dir, notice) = match header.kind {
        FileKind::File => (downloads, crate::i18n::t("pill.phone.fromPhone")),
        FileKind::Meeting => (downloads.join("Reuniones").join(meeting_folder(header.meta.as_deref())), crate::i18n::t("pill.phone.meetingFromPhone")),
    };
    if let Err(error) = std::fs::create_dir_all(&dir) {
        tracing::warn!(%error, "celular: no se pudo crear la carpeta del archivo");
        return;
    }
    let name = if header.name.trim().is_empty() { "archivo".to_string() } else { safe_name(&header.name) };
    let Some(target) = unique_path(&dir.join(name)) else {
        tracing::warn!("celular: no quedó un nombre libre para lo que mandó");
        return;
    };
    // `rename` no cruza volúmenes: si falla, copiar y borrar.
    let moved = std::fs::rename(&path, &target).or_else(|_| std::fs::copy(&path, &target).and_then(|_| std::fs::remove_file(&path)));
    if let Err(error) = moved {
        tracing::warn!(%error, "celular: no se pudo guardar lo que mandó");
        let _ = std::fs::remove_file(&target);
        return;
    }
    if header.kind == FileKind::File {
        mark_downloaded(&target);
        remember_received(&target);
    }
    let shown = target.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    // Una reunión entra también a Reuniones, donde se transcribe y resume.
    if header.kind == FileKind::Meeting {
        match crate::meetings::import_phone_meeting(header.meta.as_deref(), &dir, &target) {
            Ok(Some(title)) => {
                #[cfg(any(windows, target_os = "macos"))]
                crate::platform::tray_icon::notify(crate::i18n::t("pill.phone.meetingFromPhone"), &crate::i18n::tf("pill.phone.meetingImported", &[("title", &title)]), false, None);
                let _ = title;
                return;
            }
            Ok(None) => {}
            Err(error) => tracing::warn!(%error, "celular: no se pudo importar la reunión"),
        }
        // La otra mitad (la transcripción) llega sin aviso propio.
        if !crate::meetings::is_audio_name(&target) {
            return;
        }
    }
    // Con varias fotos a la vez, la última es la que queda para pegar.
    let copied = header.kind == FileKind::File
        && is_image(&target)
        && copy_received(&target).inspect_err(|error| tracing::warn!(%error, "celular: no se pudo copiar la foto")).is_ok();
    let key = if copied { "pill.phone.copiedAndSaved" } else { "pill.phone.savedIn" };
    #[cfg(any(windows, target_os = "macos"))]
    crate::platform::tray_icon::notify(notice, &crate::i18n::tf(key, &[("shown", &shown), ("dir", &dir.display())]), false, None);
    #[cfg(not(any(windows, target_os = "macos")))]
    let _ = (notice, shown, key);
}

/// La carpeta de una reunión del celular: «2026-10-23 Reunión con Ana», con la
/// fecha de `startedAt` (epoch en ms) y el título de su JSON.
fn meeting_folder(meta: Option<&str>) -> String {
    let meta: serde_json::Value = meta.and_then(|m| serde_json::from_str(m).ok()).unwrap_or_default();
    let title = meta.get("title").and_then(|t| t.as_str()).map(str::trim).filter(|t| !t.is_empty()).unwrap_or("Reunión");
    let date = meta
        .get("startedAt")
        .and_then(|t| t.as_i64())
        .and_then(chrono::DateTime::from_timestamp_millis)
        .map(|at| at.with_timezone(&chrono::Local).format("%Y-%m-%d %H.%M").to_string());
    safe_name(&match date {
        Some(date) => format!("{date} {title}"),
        None => title.to_string(),
    })
}

/// Sin barras ni caracteres que Windows no acepta en un nombre (ni los
/// invisibles que lo hacen leer distinto, ni CON, NUL…: lo de `atic_sync`).
fn safe_name(name: &str) -> String {
    let clean: String = name.chars().map(|c| if matches!(c, '/' | '\\') { '-' } else { c }).collect();
    atic_sync::proto::safe_file_name(&clean)
}

/// Lo que llega del celular viene de afuera: Windows (SmartScreen) y macOS
/// (Gatekeeper) avisan antes de abrirlo, como con lo que baja el navegador.
fn mark_downloaded(path: &std::path::Path) {
    #[cfg(windows)]
    {
        let mut zone = path.as_os_str().to_owned();
        zone.push(":Zone.Identifier");
        let _ = std::fs::write(zone, "[ZoneTransfer]\r\nZoneId=3\r\n");
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("/usr/bin/xattr")
            .args(["-w", "com.apple.quarantine"])
            .arg(format!("0081;{:x};Atic;", chrono::Utc::now().timestamp()))
            .arg(path)
            .stderr(std::process::Stdio::null())
            .status();
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    let _ = path;
}

/// `foto.jpg` → `foto (2).jpg` si ya existe: no pisa nada. Lo deja creado
/// vacío, para que otro archivo que llega a la vez no tome el mismo nombre.
fn unique_path(path: &std::path::Path) -> Option<PathBuf> {
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("archivo");
    let ext = path.extension().and_then(|s| s.to_str()).map(|e| format!(".{e}")).unwrap_or_default();
    let parent = path.parent().unwrap_or(std::path::Path::new("."));
    std::iter::once(path.to_path_buf())
        .chain((2..10_000).map(|n| parent.join(format!("{stem} ({n}){ext}"))))
        .find(|p| std::fs::OpenOptions::new().write(true).create_new(true).open(p).is_ok())
}

/// Lo que suena en el celular, como una fuente más de `media`.
fn show_phone_media(device_id: String, media: Option<MediaState>, art: Option<Vec<u8>>) {
    let Some(player) = MEDIA.get() else {
        return;
    };
    let track = media.filter(|m| !m.title.trim().is_empty()).map(|m| crate::media::Track {
        id: crate::media::PHONE_SOURCE.to_string(),
        title: m.title,
        artist: m.artist,
        source: crate::i18n::t("settings.nav.phone").to_string(),
        playing: m.playing,
        art: art.as_deref().and_then(crate::media::mini_art),
        ..Default::default()
    });
    *PHONE_MEDIA_DEVICE.lock_or_recover() = track.as_ref().map(|_| device_id);
    player.set_phone(track);
}

/// Pausa, siguiente o anterior en el celular que suena.
pub fn media_control(control: Control) {
    let command = match control {
        Control::Toggle => PcCommand::MediaToggle,
        Control::Next => PcCommand::MediaNext,
        Control::Previous => PcCommand::MediaPrev,
        _ => return,
    };
    let Some(device) = PHONE_MEDIA_DEVICE.lock_or_recover().clone() else {
        return;
    };
    if let Some(running) = RUNNING.lock_or_recover().as_ref() {
        running.desktop.phone_media_command(&device, command);
    }
}

/// Hace sonar el celular para encontrarlo; `false` para que se calle. `false`
/// si no está conectado o no sabe hacerlo.
pub fn ring_phone(device: &str, on: bool) -> bool {
    RUNNING.lock_or_recover().as_ref().is_some_and(|running| running.desktop.ring_phone(device, on))
}

/// Los celulares conectados que saben hacer `f`, siempre en el mismo orden.
/// Sin leer el disco: se llama al dibujar.
fn connected_with(f: &str) -> Vec<String> {
    let guard = RUNNING.lock_or_recover();
    let Some(running) = guard.as_ref() else {
        return Vec::new();
    };
    let mut ids: Vec<String> =
        running.connected.iter().filter(|id| running.desktop.phone_features(id).iter().any(|x| x == f)).cloned().collect();
    ids.sort();
    ids
}

/// Pide una foto al celular conectado (a uno, si hay varios): abre su
/// cámara y la foto llega al portapapeles. `false` si no hay a quién.
pub fn take_photo() -> bool {
    let devices = connected_with(feature::PHOTO);
    let Some(device) = devices.first() else {
        return false;
    };
    let asked = RUNNING.lock_or_recover().as_ref().is_some_and(|running| running.desktop.take_photo(device));
    if asked {
        PHOTO_ASKED.lock_or_recover().insert(device.clone(), Instant::now());
    }
    asked
}

/// A qué celular se le pidió una foto y cuándo: solo esa se pega (aunque el
/// portapapeles compartido esté apagado).
static PHOTO_ASKED: std::sync::LazyLock<Mutex<HashMap<String, Instant>>> = std::sync::LazyLock::new(Default::default);
/// Lo que se espera la foto pedida (abrir la cámara, encuadrar, sacarla).
const PHOTO_WAIT: Duration = Duration::from_secs(10 * 60);

/// Si se le pidió una foto a ese celular hace poco; la pregunta la gasta.
fn photo_was_asked(device: &str) -> bool {
    let mut asked = PHOTO_ASKED.lock_or_recover();
    asked.retain(|_, at| at.elapsed() < PHOTO_WAIT);
    asked.remove(device).is_some()
}

/// Si hay un celular conectado que sabe abrir enlaces.
pub fn can_open_links() -> bool {
    !connected_with(feature::LINKS).is_empty()
}

/// Abre el enlace en el navegador del celular conectado (en uno, si hay
/// varios). `false` si no salió.
pub fn open_on_phone(url: &str) -> bool {
    let devices = connected_with(feature::LINKS);
    let guard = RUNNING.lock_or_recover();
    devices.first().zip(guard.as_ref()).is_some_and(|(device, running)| running.desktop.open_url(device, url.trim().to_string()))
}

fn run_command(command: PcCommand) {
    let control = match command {
        PcCommand::MediaToggle => Some(Control::Toggle),
        PcCommand::MediaNext => Some(Control::Next),
        PcCommand::MediaPrev => Some(Control::Previous),
        PcCommand::StopRecording => None,
        // Desde acá, solo con `feature::CONTROL`.
        PcCommand::VolumeUp => return crate::platform::control::run(crate::platform::control::Action::VolumeUp),
        PcCommand::VolumeDown => return crate::platform::control::run(crate::platform::control::Action::VolumeDown),
        PcCommand::Mute => return crate::platform::control::run(crate::platform::control::Action::Mute),
        PcCommand::LockScreen => return crate::platform::control::run(crate::platform::control::Action::LockScreen),
        PcCommand::SlideNext => return crate::platform::control::run(crate::platform::control::Action::SlideNext),
        PcCommand::SlidePrev => return crate::platform::control::run(crate::platform::control::Action::SlidePrev),
        // Con `feature::REMOTE`.
        PcCommand::Sleep => return crate::platform::control::run(crate::platform::control::Action::Sleep),
    };
    match control {
        Some(control) => {
            if let Some(media) = MEDIA.get() {
                media.control(control);
            }
        }
        None => {
            if RECORDING.lock_or_recover().is_some() {
                if let Some(tx) = COMMANDS.get() {
                    let _ = tx.send(Command::StopRecording);
                }
            }
        }
    }
}

fn merge_from_phone(device_id: &str, items: Vec<ClipItem>, deleted: Vec<String>) {
    remember_deleted(&deleted);
    crate::clipboard_owner::with_history(|history, dir| history.delete_ids(dir, &deleted));
    let tombstones = load_tombstones();
    let mut need = Vec::new();
    let mut stale = Vec::new();
    for item in items {
        if tombstones.contains_key(&item.id) {
            stale.push(item.id);
            continue;
        }
        // El mismo texto puede estar acá con otro id (el celular lo mandó y la pill lo
        // guardó con el suyo): se busca también por el texto.
        let fingerprint = (item.kind == ClipKind::Text && !item.text.trim().is_empty())
            .then(|| atic_clipboard::fingerprint_text(item.text.trim()));
        let local = crate::clipboard_owner::with_history(|history, _| {
            history
                .items
                .iter()
                .find(|i| i.id == item.id || fingerprint.as_ref().is_some_and(|f| &i.fingerprint == f))
                .map(|i| (i.id.clone(), i.pinned))
        });
        let Some(local) = local else { continue };
        if let Some((id, pinned)) = local {
            // Fijar y desfijar también viajan: el celular trae cómo quedó cada ítem.
            let changed = pinned != item.pinned
                && crate::clipboard_owner::with_history(|history, dir| history.set_pinned(dir, &id, item.pinned)) == Some(true);
            if changed {
                PC_DIRTY.store(true, Ordering::Relaxed);
            }
            continue;
        }
        let created_at = item.copied_at_ms.max(0) as u64;
        match item.kind {
            ClipKind::Text => {
                crate::clipboard_owner::with_history(|history, dir| {
                    history.import_text(dir, &item.id, &item.text, created_at, item.pinned)
                });
            }
            ClipKind::Image => {
                PENDING_IMPORTS
                    .lock_or_recover()
                    .get_or_insert_with(HashMap::new)
                    .insert(item.id.clone(), (created_at, item.pinned));
                need.push(item.id);
            }
        }
    }
    if let Some(desktop) = RUNNING.lock_or_recover().as_ref().map(|r| r.desktop.clone()) {
        desktop.request_clip_images(device_id, need);
        desktop.clips_deleted(stale);
    }
    PC_DIRTY.store(true, Ordering::Relaxed);
}

fn load_tombstones() -> HashMap<String, i64> {
    data_file("phone-deleted.json")
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn remember_deleted(ids: &[String]) {
    if ids.is_empty() {
        return;
    }
    let mut all = load_tombstones();
    let now = chrono::Utc::now().timestamp_millis();
    let horizon = now - TOMBSTONE_DAYS * 24 * 3600 * 1000;
    all.retain(|_, at| *at >= horizon);
    for id in ids {
        all.insert(id.clone(), now);
    }
    if let (Some(path), Ok(json)) = (data_file("phone-deleted.json"), serde_json::to_string(&all)) {
        let _ = atic_core::write_atomic_str(&path, &json);
    }
}

/// El historial para el celular; vacío si el portapapeles compartido está
/// apagado. Las imágenes van en miniatura.
fn clip_history() -> Vec<ClipItem> {
    if !CLIPBOARD_ON.load(Ordering::Relaxed) {
        return Vec::new();
    }
    let items = crate::clipboard_owner::with_history(|history, _| history.recent(atic_sync::MAX_HISTORY_ITEMS))
        .unwrap_or_default();
    items
        .into_iter()
        .filter_map(|i| {
            let base = |kind, text: String, thumb, width, height| ClipItem {
                id: i.id.clone(),
                text,
                source_name: String::new(),
                copied_at_ms: i.created_at_ms as i64,
                pinned: i.pinned,
                kind,
                thumb,
                width,
                height,
            };
            match i.kind {
                atic_clipboard::ClipboardKind::Image => {
                    let (thumb, width, height) = thumbnail(&i.id, i.image_path.as_deref()?)?;
                    Some(base(ClipKind::Image, String::new(), Some(thumb), width, height))
                }
                atic_clipboard::ClipboardKind::Text => Some(base(ClipKind::Text, i.text.clone()?, None, 0, 0)),
            }
        })
        .collect()
}

fn thumbnail(id: &str, path: &str) -> Option<(Vec<u8>, u32, u32)> {
    if let Some(hit) = THUMBS.lock_or_recover().get_or_insert_with(HashMap::new).get(id) {
        return hit.clone();
    }
    let made = (|| {
        let img = image::open(path).ok()?;
        let (width, height) = (img.width(), img.height());
        let small = img.thumbnail(THUMB_PX, THUMB_PX).to_rgb8();
        let mut out = std::io::Cursor::new(Vec::new());
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 70).encode_image(&small).ok()?;
        Some((out.into_inner(), width, height))
    })();
    THUMBS.lock_or_recover().get_or_insert_with(HashMap::new).insert(id.to_string(), made.clone());
    made
}

/// La imagen entera: el PNG tal cual si es liviano; si no, reducida en JPEG.
fn full_image(id: &str) -> Option<(String, Vec<u8>)> {
    let path = crate::clipboard_owner::with_history(|history, _| {
        history.items.iter().find(|i| i.id == id).and_then(|i| i.image_path.clone())
    })??;
    let bytes = std::fs::read(&path).ok()?;
    let img = image::load_from_memory(&bytes).ok()?;
    if bytes.len() <= atic_sync::MAX_IMAGE_BYTES && img.width().max(img.height()) <= FULL_MAX_PX {
        return Some(("image/png".into(), bytes));
    }
    let resized = img.thumbnail(FULL_MAX_PX, FULL_MAX_PX).to_rgb8();
    let mut out = std::io::Cursor::new(Vec::new());
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 88).encode_image(&resized).ok()?;
    Some(("image/jpeg".into(), out.into_inner()))
}

/// Una imagen del celular al portapapeles del PC; el historial la recoge solo.
fn paste_image(data: &[u8]) -> Result<(), String> {
    let rgba = image::load_from_memory(data).map_err(|e| e.to_string())?.to_rgba8();
    let (w, h) = (rgba.width() as usize, rgba.height() as usize);
    atic_clipboard::set_system_image(w, h, rgba.into_raw())
}

/// Lo que suena en el PC y si la pill está grabando.
fn pc_state() -> PcState {
    let media = MEDIA.get().and_then(|media| media.pc_track()).filter(|t| !t.title.trim().is_empty()).map(|t| MediaState {
        title: t.title,
        artist: t.artist,
        app: crate::media::source_name(&t.source),
        playing: t.playing,
        can_toggle: true,
        can_next: true,
        can_prev: true,
    });
    let recording = RECORDING.lock_or_recover().map(|started_at_ms| RecordingState { started_at_ms });
    PcState { media, recording }
}

/// Lo que muestra el notch de Agentes, para el celular: las sesiones en curso
/// y los permisos que esperan. Lo llama el notch en cada barrido.
/// Conversaciones quietas de Atic Code que también van al celular («Recientes»).
const MAX_RECENT: usize = 12;

/// Lo que ve el celular: las sesiones en curso o recién terminadas, las que
/// esperan un permiso y las demás conversaciones abiertas en Atic Code
/// (`links`), con su título y si quedó algo sin leer.
pub fn set_agents(sessions: &[crate::agents::Session], waiting: &[crate::agent_prompts::Waiting], links: &[crate::code::link::ChatLink]) {
    let mut cards: Vec<AgentCard> = sessions
        .iter()
        .map(|session| {
            let agent = &crate::agents::AGENTS[session.agent];
            let link = crate::code::link::link_for(links, &session.id);
            AgentCard {
                id: session.id.clone(),
                backend_id: agent.cli.to_string(),
                backend_name: agent.name.to_string(),
                project: crate::agents::folder_name(&session.cwd),
                status: match session.status {
                    crate::agents::Status::Working => AgentStatus::Working,
                    crate::agents::Status::Ready => AgentStatus::Ready,
                },
                activity: session.activity.clone().map(|detail| AgentActivity {
                    kind: ActivityKind::Tool,
                    detail: Some(detail),
                }),
                preview: session.preview.clone(),
                permission: None,
                title: link.map(|l| l.title.clone()).filter(|t| !t.trim().is_empty()),
                unread: link.is_some_and(|l| l.unread),
                model: link.and_then(|l| l.model.clone()),
                updated_ms: Some(session.updated * 1000),
                chat: link.is_some(),
            }
        })
        .collect();
    // Las conversaciones sin actividad reciente: una tarjeta por conversación
    // (una de un agente ACP trae dos sesiones).
    let mut keys: HashSet<&str> = links.iter().filter(|l| cards.iter().any(|c| c.id == l.session)).map(|l| l.key.as_str()).collect();
    for link in links {
        if cards.len() >= sessions.len() + MAX_RECENT || !keys.insert(link.key.as_str()) {
            continue;
        }
        let cli = link.acp.unwrap_or("claude");
        let name = crate::agents::AGENTS.iter().find(|a| a.cli == cli).map_or(cli, |a| a.name);
        cards.push(AgentCard {
            id: link.session.clone(),
            backend_id: cli.to_string(),
            backend_name: name.to_string(),
            project: crate::agents::folder_name(&link.cwd.to_string_lossy()),
            status: if link.busy { AgentStatus::Working } else if link.unread { AgentStatus::Ready } else { AgentStatus::Idle },
            activity: None,
            preview: None,
            permission: None,
            title: Some(link.title.clone()).filter(|t| !t.trim().is_empty()),
            unread: link.unread,
            model: link.model.clone(),
            updated_ms: None,
            chat: true,
        });
    }
    for ask in waiting {
        let permission = PermissionAsk {
            id: ask.id.clone(),
            title: ask.tool.clone(),
            detail: Some(ask.detail.clone()).filter(|detail| !detail.is_empty()),
            can_allow_always: false,
            questions: Vec::new(),
        };
        match cards.iter_mut().find(|card| card.id == ask.session) {
            Some(card) => {
                card.status = AgentStatus::Waiting;
                card.permission = Some(permission);
            }
            None => {
                let agent = crate::agents::AGENTS.iter().find(|agent| agent.cli == ask.agent);
                cards.push(AgentCard {
                    id: ask.session.clone(),
                    backend_id: ask.agent.to_string(),
                    backend_name: agent.map_or(ask.agent, |agent| agent.name).to_string(),
                    project: String::new(),
                    status: AgentStatus::Waiting,
                    activity: None,
                    preview: None,
                    permission: Some(permission),
                    title: None,
                    unread: false,
                    model: None,
                    updated_ms: None,
                    chat: false,
                });
            }
        }
    }
    *AGENTS.lock_or_recover() = Some(cards);
}

async fn publish_loop(desktop: Arc<Desktop>) {
    let mut last_pc = Instant::now() - PC_REFRESH;
    let mut last_state: Option<PcState> = None;
    let mut last_agents: Option<Vec<AgentCard>> = None;
    loop {
        tokio::time::sleep(TICK).await;
        let agents = AGENTS.lock_or_recover().clone().unwrap_or_default();
        if last_agents.as_ref() != Some(&agents) {
            desktop.publish(agents.clone());
            last_agents = Some(agents);
        }
        if PC_DIRTY.swap(false, Ordering::Relaxed) || last_pc.elapsed() >= PC_REFRESH {
            last_pc = Instant::now();
            let state = pc_state();
            if last_state.as_ref() != Some(&state) {
                desktop.publish_pc(state.clone());
                last_state = Some(state);
            }
            let history = tokio::task::spawn_blocking(clip_history).await.unwrap_or_default();
            desktop.publish_clip_history(history);
        }
    }
}

fn load_or_create_key() -> Result<[u8; 32], String> {
    if let Some(key) = dev_key::load() {
        return Ok(key);
    }
    if let Some(hex) = atic_core::secrets::get_phone_sync_key().map_err(|e| e.to_string())? {
        if let Some(key) = decode_hex(&hex) {
            dev_key::save(&hex);
            return Ok(key);
        }
    }
    let key = atic_sync::generate_secret_key();
    let hex: String = key.iter().map(|b| format!("{b:02x}")).collect();
    atic_core::secrets::set_phone_sync_key(&hex).map_err(|e| e.to_string())?;
    dev_key::save(&hex);
    Ok(key)
}

/// Solo en debug: una copia de la clave del celular en un archivo que solo
/// lee el usuario (`phone-key.dev`). Cada compilación tiene otro hash y el
/// llavero de macOS la trata como una app nueva (sin Team ID, la partición es
/// el `cdhash`): sin esto, pedía permiso en cada build y la pill no se
/// conectaba hasta contestarle. En release la clave vive solo en el llavero.
mod dev_key {
    #[cfg(debug_assertions)]
    fn path() -> Option<std::path::PathBuf> {
        super::data_file("phone-key.dev")
    }

    #[cfg(debug_assertions)]
    pub fn load() -> Option<[u8; 32]> {
        let hex = std::fs::read_to_string(path()?).ok()?;
        super::decode_hex(hex.trim())
    }

    #[cfg(debug_assertions)]
    pub fn save(hex: &str) {
        let Some(path) = path() else {
            return;
        };
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        let written = options.open(&path).and_then(|mut file| std::io::Write::write_all(&mut file, hex.as_bytes()));
        if let Err(err) = written {
            tracing::warn!("celular: no se pudo guardar la clave de debug: {err}");
        }
    }

    #[cfg(not(debug_assertions))]
    pub fn load() -> Option<[u8; 32]> {
        None
    }

    #[cfg(not(debug_assertions))]
    pub fn save(_: &str) {}
}

fn decode_hex(hex: &str) -> Option<[u8; 32]> {
    let bytes: Vec<u8> = (0..hex.len())
        .step_by(2)
        .map(|i| hex.get(i..i + 2).and_then(|b| u8::from_str_radix(b, 16).ok()))
        .collect::<Option<_>>()?;
    bytes.try_into().ok()
}

/// Cómo aparece este PC en el celular.
fn desktop_name() -> String {
    computer_name().map(|n| format!("Atic en {n}")).unwrap_or_else(|| "Atic".into())
}

/// El nombre del equipo: `COMPUTERNAME` en Windows; en macOS, el de Compartir
/// (`scutil --get ComputerName`), que es el que el usuario reconoce.
fn computer_name() -> Option<String> {
    if let Some(name) = std::env::var("COMPUTERNAME").ok().filter(|n| !n.trim().is_empty()) {
        return Some(name);
    }
    #[cfg(target_os = "macos")]
    {
        let out = std::process::Command::new("/usr/sbin/scutil").args(["--get", "ComputerName"]).output().ok()?;
        let name = String::from_utf8(out.stdout).ok()?.trim().to_string();
        if out.status.success() && !name.is_empty() {
            return Some(name);
        }
    }
    None
}

/// El QR del ticket, como imagen lista para GPUI (gris: BGRA y RGBA coinciden).
pub fn qr_image(ticket: &str) -> Option<Arc<gpui::RenderImage>> {
    let code = qrcode::QrCode::new(ticket.as_bytes()).ok()?;
    let gray = code.render::<image::Luma<u8>>().quiet_zone(true).min_dimensions(240, 240).build();
    let rgba = image::DynamicImage::ImageLuma8(gray).to_rgba8();
    Some(Arc::new(gpui::RenderImage::new([image::Frame::new(rgba)])))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_clave_va_y_vuelve_en_hex() {
        let key = atic_sync::generate_secret_key();
        let hex: String = key.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(decode_hex(&hex), Some(key));
        assert_eq!(decode_hex("zz"), None);
    }

    #[test]
    fn el_qr_se_arma() {
        assert!(qr_image("ticket-de-prueba").is_some());
    }

    #[test]
    fn la_reunion_del_celular_va_a_su_carpeta() {
        // 2026-10-23 14:05 UTC; la hora sale en la zona del PC, la fecha y el título siempre.
        let meta = r#"{"id":"m1","title":"Reunión con Ana","startedAt":1792764300000,"durationSecs":60}"#;
        let folder = meeting_folder(Some(meta));
        assert!(folder.starts_with("2026-10-2"), "{folder}");
        assert!(folder.ends_with(" Reunión con Ana"), "{folder}");
        // Sin JSON o sin título: «Reunión».
        assert_eq!(meeting_folder(None), "Reunión");
        assert_eq!(meeting_folder(Some(r#"{"title":"  "}"#)), "Reunión");
    }

    #[test]
    fn los_nombres_del_celular_no_traen_carpetas() {
        assert_eq!(safe_name("../../etc/passwd"), "-..-etc-passwd");
        assert_eq!(safe_name("acta: final?.pdf"), "acta final.pdf");
        assert_eq!(safe_name("   "), "archivo");
        assert_eq!(safe_name("foto\u{202E}gpj.exe"), "fotogpj.exe");
        assert_eq!(safe_name("CON.txt"), "_CON.txt");
    }

    #[test]
    fn el_celular_ve_las_conversaciones_con_titulo_y_las_quietas_en_recientes() {
        use crate::code::link::ChatLink;
        let link = |session: &str, key: &str, unread: bool| ChatLink {
            session: session.into(),
            key: key.into(),
            in_view: false,
            title: format!("Chat {key}"),
            cwd: PathBuf::from("/repos/atic"),
            unread,
            busy: false,
            model: Some("opus".into()),
            acp: None,
        };
        let session = crate::agents::Session {
            id: "s1".into(),
            agent: 0,
            cwd: "/repos/atic".into(),
            status: crate::agents::Status::Ready,
            preview: Some("Listo".into()),
            activity: None,
            updated: 1_700_000_000,
        };
        // c3 es de un agente ACP: dos sesiones, una tarjeta.
        let links = [link("s1", "c1", true), link("s2", "c2", false), link("s3", "c3", false), link("t3", "c3", false)];
        set_agents(&[session], &[], &links);
        let cards = AGENTS.lock_or_recover().clone().unwrap();
        assert_eq!(cards.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), vec!["s1", "s2", "s3"]);
        assert_eq!((cards[0].title.as_deref(), cards[0].unread, cards[0].updated_ms), (Some("Chat c1"), true, Some(1_700_000_000_000)));
        assert_eq!((cards[1].status, cards[1].project.as_str()), (AgentStatus::Idle, "atic"));
        assert!(cards.iter().all(|c| c.chat));
    }

    #[test]
    fn lo_que_llega_no_pisa_nada() {
        let dir = std::env::temp_dir().join(format!("atic-phone-inbox-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // Cada nombre queda tomado al elegirlo: dos que llegan a la vez no chocan.
        let first = dir.join("foto.jpg");
        assert_eq!(unique_path(&first), Some(first.clone()));
        assert_eq!(unique_path(&first), Some(dir.join("foto (2).jpg")));
        let bare = dir.join("notas");
        std::fs::write(&bare, b"x").unwrap();
        assert_eq!(unique_path(&bare), Some(dir.join("notas (2)")));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
