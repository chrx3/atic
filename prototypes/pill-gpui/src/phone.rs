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
use atic_sync::desktop::{Desktop, DesktopConfig, DesktopEvent};
use atic_sync::{
    ActivityKind, AgentActivity, AgentCard, AgentStatus, ClipItem, ClipKind, Decision, MediaState, PcCommand,
    PcState, PermissionAsk, RecordingState,
};
use serde::{Deserialize, Serialize};

use crate::media::{Control, Media};

/// Lo que el celular le pide a la ventana de la pill.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    StopRecording,
}

const TICK: Duration = Duration::from_millis(250);
const PC_REFRESH: Duration = Duration::from_secs(2);
const TOMBSTONE_DAYS: i64 = 60;
const THUMB_PX: u32 = 200;
const FULL_MAX_PX: u32 = 2560;

static RUNTIME: OnceLock<Option<tokio::runtime::Runtime>> = OnceLock::new();
static RUNNING: Mutex<Option<Running>> = Mutex::new(None);
/// Evita dos arranques a la vez (el del inicio y el de un QR pedido enseguida).
static STARTING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
static CLIPBOARD_ON: AtomicBool = AtomicBool::new(false);
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
    let devices: Devices = data_file("phone.json")
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default();
    CLIPBOARD_ON.store(devices.clipboard, Ordering::Relaxed);
    devices
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
    let native = AppDirs::new().is_ok_and(|dirs| Config::load(&dirs.config_path()).native_pill);
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
    let _starting = STARTING.lock().await;
    if let Some(running) = RUNNING.lock_or_recover().as_ref() {
        return Ok(running.desktop.clone());
    }
    let trusted = load_devices().devices.into_iter().map(|d| d.id).collect();
    let (desktop, events) = Desktop::start(DesktopConfig { secret_key: load_or_create_key()?, name: desktop_name(), trusted })
        .await
        .map_err(|e| format!("{e:#}"))?;
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
            DesktopEvent::Connected { device_id, .. } => {
                if let Some(running) = RUNNING.lock_or_recover().as_mut() {
                    running.connected.insert(device_id);
                }
            }
            DesktopEvent::Disconnected { device_id } => {
                if let Some(running) = RUNNING.lock_or_recover().as_mut() {
                    running.connected.remove(&device_id);
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
            // Las preguntas con opciones se contestan en la consola, y la
            // música del celular la pill todavía no la muestra.
            _ => {}
        }
    }
}

fn run_command(command: PcCommand) {
    let control = match command {
        PcCommand::MediaToggle => Some(Control::Toggle),
        PcCommand::MediaNext => Some(Control::Next),
        PcCommand::MediaPrev => Some(Control::Previous),
        PcCommand::StopRecording => None,
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
        let known = crate::clipboard_owner::with_history(|history, _| history.items.iter().any(|i| i.id == item.id));
        if known != Some(false) {
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
    let media = MEDIA.get().and_then(|media| media.track()).filter(|t| !t.title.trim().is_empty()).map(|t| MediaState {
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
pub fn set_agents(sessions: &[crate::agents::Session], waiting: &[crate::agent_prompts::Waiting]) {
    let mut cards: Vec<AgentCard> = sessions
        .iter()
        .map(|session| {
            let agent = &crate::agents::AGENTS[session.agent];
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
            }
        })
        .collect();
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
    if let Some(hex) = atic_core::secrets::get_phone_sync_key().map_err(|e| e.to_string())? {
        if let Some(key) = decode_hex(&hex) {
            return Ok(key);
        }
    }
    let key = atic_sync::generate_secret_key();
    let hex: String = key.iter().map(|b| format!("{b:02x}")).collect();
    atic_core::secrets::set_phone_sync_key(&hex).map_err(|e| e.to_string())?;
    Ok(key)
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
    std::env::var("COMPUTERNAME")
        .map(|n| format!("Atic en {n}"))
        .unwrap_or_else(|_| "Atic".into())
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
}
