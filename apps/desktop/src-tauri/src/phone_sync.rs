//! Canal con la app del celular (`atic-sync`): publica los agentes y recibe
//! las respuestas a permisos.
//!
//! Apagado por defecto, como pide `PRODUCT.md`: el canal solo se abre si ya
//! hay celulares pareados o si alguien pide un QR en Ajustes → Celular.
//!
//! La foto junta dos fuentes que el resto de la app tiene separadas: las
//! sesiones que maneja Atic (las únicas con permisos contestables) y la
//! presencia de los CLIs que corren por fuera.
//!
//! Un celular con el QR no entra solo: el pareo se confirma acá
//! ([`phone_pair_answer`]). El portapapeles también es aparte y empieza
//! apagado: cada tipo de dato se activa a propósito.

use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

use atic_sync::{
    desktop::{Desktop, DesktopConfig, DesktopEvent},
    ActivityKind, AgentActivity, AgentCard, AgentStatus, ClipItem, ClipKind, Decision, MediaState, PcCommand, PcState, PermissionAsk,
    RecordingState,
};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

use atic_core::sync::MutexExt;

use crate::{
    agents::{self, presence, PermissionDecision},
    state::AppState,
};

/// Algo cambió en agentes o permisos: la próxima vuelta reconstruye la foto.
static DIRTY: AtomicBool = AtomicBool::new(true);
/// Copia en memoria de `Devices::clipboard`: el vigilante corre seguido y no
/// tiene `AppHandle` para leer el archivo.
static CLIPBOARD_ON: AtomicBool = AtomicBool::new(false);
static RUNNING: Mutex<Option<Running>> = Mutex::new(None);
/// Para lo que llama el historial sin `AppHandle` (borrados a mano).
static APP: Mutex<Option<AppHandle>> = Mutex::new(None);
/// Evita dos arranques a la vez (el del inicio y el de un QR pedido enseguida).
static STARTING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
/// Ítems borrados en el PC (o en el celular), con cuándo. Sin esto, un celular
/// que se perdió el aviso volvería a ofrecerlos al conectarse y revivirían.
static TOMBSTONES: Mutex<Option<HashMap<String, i64>>> = Mutex::new(None);
/// Cuánto se recuerda un borrado.
const TOMBSTONE_DAYS: i64 = 60;
/// Imágenes que el celular ofreció y el PC pidió: su fecha y si van fijadas.
static PENDING_IMPORTS: Mutex<Option<HashMap<String, (u64, bool)>>> = Mutex::new(None);

/// Lo que suena en el celular, para la pill (ver `media::media_now`).
static PHONE_MEDIA: Mutex<Option<PhoneMedia>> = Mutex::new(None);

struct PhoneMedia {
    device_id: String,
    media: MediaState,
    /// Carátula como data URL, lista para la pill.
    art: Option<String>,
}

/// Lo último que llegó del celular al portapapeles. El vigilante del historial
/// lo va a ver como una copia nueva; sin esto se lo devolveríamos como eco.
static FROM_PHONE: Mutex<Option<String>> = Mutex::new(None);

/// Cada cuánto se mira si hay que publicar. Los cambios se marcan con [`poke`].
const TICK: Duration = Duration::from_millis(250);
/// Aunque nadie avise, se revisa cada tanto: la presencia degrada sola.
const FULL_REFRESH: Duration = Duration::from_secs(3);
/// Cada cuánto se pregunta qué suena. La pill sondea a un ritmo parecido.
const PC_REFRESH: Duration = Duration::from_secs(2);
/// Un comando del celular cambió medios o grabación: mirar ya, sin esperar.
static PC_DIRTY: AtomicBool = AtomicBool::new(false);

struct Running {
    desktop: Arc<Desktop>,
    connected: HashSet<String>,
    /// Un celular con ticket válido esperando que alguien lo acepte.
    pending: Option<PendingPair>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PendingPair {
    device_id: String,
    device_name: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Devices {
    devices: Vec<Device>,
    /// Compartir el portapapeles de texto con los celulares pareados.
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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PhoneStatus {
    running: bool,
    devices: Vec<DeviceView>,
    clipboard: bool,
    pending_pair: Option<PendingPair>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DeviceView {
    id: String,
    name: String,
    paired_at: i64,
    connected: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PhonePairing {
    ticket: String,
    qr_svg: String,
    expires_in_secs: u64,
}

/// Marca la foto como vieja. Barato y sin locks: lo llaman `on_delta` de cada
/// sesión y `presence::publish`, que corren seguido.
pub fn poke() {
    DIRTY.store(true, Ordering::Relaxed);
}

/// Al arrancar Atic: abre el canal solo si ya hay celulares pareados.
pub fn start_if_paired(app: &AppHandle) {
    if load_devices(app).devices.is_empty() {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(err) = ensure_running(&app).await {
            tracing::warn!(%err, "no se pudo abrir el canal con el celular");
        }
    });
}

#[tauri::command]
pub fn phone_status(app: AppHandle) -> PhoneStatus {
    status(&app)
}

/// Abre una ventana de pareo de 5 minutos y devuelve el QR.
#[tauri::command]
pub async fn phone_pair_start(app: AppHandle) -> Result<PhonePairing, String> {
    let desktop = ensure_running(&app).await?;
    let ticket = desktop.pairing_ticket().await;
    let qr_svg = qrcode::QrCode::new(ticket.as_bytes())
        .map_err(|e| e.to_string())?
        .render::<qrcode::render::svg::Color>()
        .quiet_zone(true)
        .min_dimensions(240, 240)
        .build();
    Ok(PhonePairing { ticket, qr_svg, expires_in_secs: 300 })
}

#[tauri::command]
pub fn phone_pair_cancel() {
    if let Some(running) = RUNNING.lock_or_recover().as_ref() {
        running.desktop.cancel_pairing();
    }
}

/// Acepta o rechaza al celular que escaneó el QR.
#[tauri::command]
pub fn phone_pair_answer(app: AppHandle, device_id: String, accept: bool) -> Result<(), String> {
    let desktop = {
        let mut guard = RUNNING.lock_or_recover();
        let running = guard.as_mut().ok_or("el canal con el celular no está abierto")?;
        running.pending = None;
        running.desktop.clone()
    };
    let delivered = desktop.approve_pairing(&device_id, accept);
    emit_status(&app);
    if delivered {
        Ok(())
    } else {
        Err("Ese pedido ya venció. Muestra el QR de nuevo.".into())
    }
}

#[tauri::command]
pub fn phone_set_clipboard(app: AppHandle, enabled: bool) -> Result<(), String> {
    let mut devices = load_devices(&app);
    devices.clipboard = enabled;
    save_devices(&app, &devices)?;
    CLIPBOARD_ON.store(enabled, Ordering::Relaxed);
    // Al apagarlo, el celular deja de ver el historial en la próxima vuelta.
    PC_DIRTY.store(true, Ordering::Relaxed);
    emit_status(&app);
    Ok(())
}

/// El celular trae su historial: se borra lo que borró, se suma lo que falta.
/// Lo que el PC borró y el celular todavía tiene, se le avisa para que lo borre.
fn merge_from_phone(app: &AppHandle, device_id: &str, items: Vec<ClipItem>, deleted: Vec<String>) {
    remember_deleted(app, &deleted);
    crate::clipboard_history::delete_ids(app, &deleted);
    let tombstones = load_tombstones(app);
    let mut need = Vec::new();
    let mut stale = Vec::new();
    for item in items {
        if tombstones.contains_key(&item.id) {
            stale.push(item.id);
            continue;
        }
        if crate::clipboard_history::has_item(&item.id) {
            continue;
        }
        let created_at = item.copied_at_ms.max(0) as u64;
        match item.kind {
            ClipKind::Text => {
                crate::clipboard_history::import_text(app, &item.id, &item.text, created_at, item.pinned);
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
    let Some(desktop) = RUNNING.lock_or_recover().as_ref().map(|r| r.desktop.clone()) else {
        return;
    };
    desktop.request_clip_images(device_id, need);
    desktop.clips_deleted(stale);
    PC_DIRTY.store(true, Ordering::Relaxed);
}

/// Lo llama el historial al borrar a mano: el celular también lo borra, y lo
/// recuerda para los otros PCs.
pub(crate) fn clips_deleted_on_pc(ids: Vec<String>) {
    if ids.is_empty() {
        return;
    }
    let desktop = RUNNING.lock_or_recover().as_ref().map(|r| r.desktop.clone());
    if let Some(desktop) = desktop {
        if let Some(app) = APP.lock_or_recover().clone() {
            remember_deleted(&app, &ids);
        }
        desktop.clips_deleted(ids);
    }
}

fn tombstones_path(app: &AppHandle) -> PathBuf {
    app.state::<AppState>().dirs.config_path().with_file_name("phone-deleted.json")
}

fn load_tombstones(app: &AppHandle) -> HashMap<String, i64> {
    let mut guard = TOMBSTONES.lock_or_recover();
    if guard.is_none() {
        let loaded: HashMap<String, i64> = std::fs::read_to_string(tombstones_path(app))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        *guard = Some(loaded);
    }
    guard.clone().unwrap_or_default()
}

fn remember_deleted(app: &AppHandle, ids: &[String]) {
    if ids.is_empty() {
        return;
    }
    let mut all = load_tombstones(app);
    let now = chrono::Utc::now().timestamp_millis();
    let horizon = now - TOMBSTONE_DAYS * 24 * 3600 * 1000;
    all.retain(|_, at| *at >= horizon);
    for id in ids {
        all.insert(id.clone(), now);
    }
    if let Ok(json) = serde_json::to_string(&all) {
        let _ = atic_core::fs_atomic::write_atomic_str(&tombstones_path(app), &json);
    }
    *TOMBSTONES.lock_or_recover() = Some(all);
}

/// Lo que suena en el celular, en la forma que entiende la pill.
pub(crate) fn phone_media_now() -> Option<crate::media::MediaNow> {
    let guard = PHONE_MEDIA.lock_or_recover();
    let now = guard.as_ref()?;
    let m = &now.media;
    Some(crate::media::MediaNow {
        title: m.title.clone(),
        artist: m.artist.clone(),
        app: format!("{} · celular", m.app),
        playing: m.playing,
        can_toggle: m.can_toggle,
        can_next: m.can_next,
        can_prev: m.can_prev,
        can_seek: false,
        thumbnail: now.art.clone(),
        thumb_key: format!("phone:{}:{}:{}", now.device_id, m.title, m.artist),
        position_ms: None,
        duration_ms: None,
        updated_ms: None,
    })
}

/// Play/pausa, siguiente o anterior para la música del celular. `false` si no
/// hay celular con música.
pub(crate) fn phone_media_control(action: &str) -> bool {
    let command = match action {
        "toggle" => PcCommand::MediaToggle,
        "next" => PcCommand::MediaNext,
        "prev" => PcCommand::MediaPrev,
        _ => return false,
    };
    let Some(device_id) = PHONE_MEDIA.lock_or_recover().as_ref().map(|m| m.device_id.clone()) else {
        return false;
    };
    let Some(desktop) = RUNNING.lock_or_recover().as_ref().map(|r| r.desktop.clone()) else {
        return false;
    };
    desktop.phone_media_command(&device_id, command)
}

/// Lo llama el vigilante del historial con cada texto nuevo (ya filtrado lo
/// sensible). Sale solo si el portapapeles compartido está activo.
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
    let Some(desktop) = RUNNING.lock_or_recover().as_ref().map(|r| r.desktop.clone()) else {
        return;
    };
    desktop.send_clip(text.to_string(), chrono::Utc::now().timestamp_millis());
}

#[tauri::command]
pub fn phone_unpair(app: AppHandle, device_id: String) -> Result<(), String> {
    let mut devices = load_devices(&app);
    devices.devices.retain(|d| d.id != device_id);
    save_devices(&app, &devices)?;
    if let Some(running) = RUNNING.lock_or_recover().as_ref() {
        running.desktop.revoke(&device_id).map_err(|e| e.to_string())?;
    }
    emit_status(&app);
    Ok(())
}

async fn ensure_running(app: &AppHandle) -> Result<Arc<Desktop>, String> {
    let _starting = STARTING.lock().await;
    *APP.lock_or_recover() = Some(app.clone());
    if let Some(running) = RUNNING.lock_or_recover().as_ref() {
        return Ok(running.desktop.clone());
    }
    let trusted = load_devices(app).devices.into_iter().map(|d| d.id).collect();
    let (desktop, events) = Desktop::start(DesktopConfig {
        secret_key: load_or_create_key()?,
        name: desktop_name(),
        trusted,
    })
    .await
    .map_err(|e| format!("{e:#}"))?;
    let desktop = Arc::new(desktop);
    *RUNNING.lock_or_recover() = Some(Running {
        desktop: desktop.clone(),
        connected: HashSet::new(),
        pending: None,
    });

    tauri::async_runtime::spawn(handle_events(app.clone(), events));
    tauri::async_runtime::spawn(publish_loop(app.clone(), desktop.clone()));
    emit_status(app);
    Ok(desktop)
}

async fn handle_events(app: AppHandle, mut events: tokio::sync::mpsc::UnboundedReceiver<DesktopEvent>) {
    while let Some(event) = events.recv().await {
        match event {
            DesktopEvent::PairRequested { device_id, device_name } => {
                if let Some(running) = RUNNING.lock_or_recover().as_mut() {
                    running.pending = Some(PendingPair { device_id, device_name });
                }
                // El QR se pide en Ajustes → Celular, que muestra la pregunta.
                show_main_window(&app);
            }
            DesktopEvent::Clip { item, .. } => {
                if !CLIPBOARD_ON.load(Ordering::Relaxed) {
                    continue;
                }
                let text = item.text.trim().to_string();
                *FROM_PHONE.lock_or_recover() = Some(text.clone());
                if let Err(err) = crate::clipboard_history::set_system_text(text) {
                    tracing::warn!(%err, "no se pudo pegar lo que mandó el celular");
                }
                continue;
            }
            DesktopEvent::ClipSync { device_id, items, deleted } => {
                if CLIPBOARD_ON.load(Ordering::Relaxed) {
                    merge_from_phone(&app, &device_id, items, deleted);
                }
                continue;
            }
            DesktopEvent::ClipImage { id, data, .. } => {
                let meta = PENDING_IMPORTS.lock_or_recover().get_or_insert_with(HashMap::new).remove(&id);
                if let (Some((created_at, pinned)), true) = (meta, CLIPBOARD_ON.load(Ordering::Relaxed)) {
                    let app = app.clone();
                    let imported = tauri::async_runtime::spawn_blocking(move || {
                        crate::clipboard_history::import_image(&app, &id, &data, created_at, pinned)
                    })
                    .await;
                    if let Ok(Err(err)) = imported {
                        tracing::warn!(%err, "no se pudo sumar una imagen del celular al historial");
                    }
                    PC_DIRTY.store(true, Ordering::Relaxed);
                }
                continue;
            }
            DesktopEvent::ClipDelete { ids, .. } => {
                remember_deleted(&app, &ids);
                crate::clipboard_history::delete_ids(&app, &ids);
                PC_DIRTY.store(true, Ordering::Relaxed);
                continue;
            }
            DesktopEvent::PhoneMedia { device_id, media, art } => {
                use base64::Engine;
                *PHONE_MEDIA.lock_or_recover() = media.map(|media| PhoneMedia {
                    device_id,
                    media,
                    art: art.map(|a| {
                        format!("data:image/jpeg;base64,{}", base64::engine::general_purpose::STANDARD.encode(a))
                    }),
                });
                continue;
            }
            DesktopEvent::FetchImage { device_id, id } => {
                let desktop = RUNNING.lock_or_recover().as_ref().map(|r| r.desktop.clone());
                if let Some(desktop) = desktop {
                    let found = tauri::async_runtime::spawn_blocking({
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
                continue;
            }
            DesktopEvent::Image { data, .. } => {
                if CLIPBOARD_ON.load(Ordering::Relaxed) {
                    let pasted = tauri::async_runtime::spawn_blocking(move || paste_phone_image(&data)).await;
                    if let Ok(Err(err)) = pasted {
                        tracing::warn!(%err, "no se pudo pegar la imagen que mandó el celular");
                    }
                }
                continue;
            }
            DesktopEvent::Command { command, .. } => {
                run_command(&app, command).await;
                PC_DIRTY.store(true, Ordering::Relaxed);
                continue;
            }
            DesktopEvent::Paired { device_id, device_name } => {
                let mut devices = load_devices(&app);
                devices.devices.retain(|d| d.id != device_id);
                devices.devices.push(Device {
                    id: device_id,
                    name: device_name,
                    paired_at: chrono::Utc::now().timestamp_millis(),
                });
                if let Err(err) = save_devices(&app, &devices) {
                    tracing::warn!(%err, "no se pudo guardar el celular pareado");
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
                // Si el celular se fue, su música deja de estar en la pill.
                let mut phone_media = PHONE_MEDIA.lock_or_recover();
                if phone_media.as_ref().is_some_and(|m| m.device_id == device_id) {
                    *phone_media = None;
                }
            }
            DesktopEvent::Decide { agent_id, permission_id, decision, .. } => {
                let decision = match decision {
                    Decision::Allow => PermissionDecision::Allow,
                    Decision::AllowAlways => PermissionDecision::AllowAlways,
                    Decision::Deny => PermissionDecision::Deny,
                };
                // Si el permiso ya se contestó en el PC, esto falla y no pasa nada:
                // la foto siguiente le muestra al celular que ya no está.
                if let Err(err) = agents::bridge::agent_permission(agent_id, permission_id, decision) {
                    tracing::info!(%err, "permiso del celular sin efecto");
                }
                poke();
                continue;
            }
        }
        emit_status(&app);
    }
}

async fn run_command(app: &AppHandle, command: PcCommand) {
    let media = match command {
        PcCommand::MediaToggle => Some("toggle"),
        PcCommand::MediaNext => Some("next"),
        PcCommand::MediaPrev => Some("prev"),
        PcCommand::StopRecording => None,
    };
    if let Some(action) = media {
        if let Err(err) = crate::media::media_control(action.into()).await {
            tracing::info!(%err, "comando de medios del celular sin efecto");
        }
        return;
    }
    let recording = app.state::<AppState>().active.lock_or_recover().is_some();
    if recording {
        if let Err(err) = crate::state::stop_capture(app) {
            tracing::warn!(%err, "no se pudo detener la grabación desde el celular");
        }
    }
}

/// El historial para el celular; vacío si el portapapeles compartido está apagado.
/// Las imágenes van como miniatura (se calcula una vez por ítem).
fn clip_history() -> Vec<ClipItem> {
    if !CLIPBOARD_ON.load(Ordering::Relaxed) {
        return Vec::new();
    }
    crate::clipboard_history::recent_items(atic_sync::MAX_HISTORY_ITEMS)
        .into_iter()
        .filter_map(|i| {
            if crate::clipboard_history::is_image(&i) {
                let (thumb, width, height) = thumbnail(&i.id, i.image_path.as_deref()?)?;
                return Some(ClipItem {
                    id: i.id,
                    text: String::new(),
                    source_name: String::new(),
                    copied_at_ms: i.created_at_ms as i64,
                    pinned: i.pinned,
                    kind: ClipKind::Image,
                    thumb: Some(thumb),
                    width,
                    height,
                });
            }
            Some(ClipItem {
                text: i.text?,
                id: i.id,
                source_name: String::new(),
                copied_at_ms: i.created_at_ms as i64,
                pinned: i.pinned,
                kind: ClipKind::Text,
                thumb: None,
                width: 0,
                height: 0,
            })
        })
        .collect()
}

/// Miniaturas ya calculadas, por id del ítem. `None` si la imagen no se pudo leer.
static THUMBS: Mutex<Option<HashMap<String, Option<(Vec<u8>, u32, u32)>>>> = Mutex::new(None);
const THUMB_PX: u32 = 200;
/// Más que esto se reduce antes de mandarla entera.
const FULL_MAX_PX: u32 = 2560;

fn thumbnail(id: &str, path: &str) -> Option<(Vec<u8>, u32, u32)> {
    if let Some(hit) = THUMBS.lock_or_recover().get_or_insert_with(HashMap::new).get(id) {
        return hit.clone();
    }
    let made = (|| {
        let img = image::open(path).ok()?;
        let (width, height) = (img.width(), img.height());
        let small = img.thumbnail(THUMB_PX, THUMB_PX).to_rgb8();
        let mut out = std::io::Cursor::new(Vec::new());
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 70)
            .encode_image(&small)
            .ok()?;
        Some((out.into_inner(), width, height))
    })();
    THUMBS.lock_or_recover().get_or_insert_with(HashMap::new).insert(id.to_string(), made.clone());
    made
}

/// La imagen entera para el celular: el PNG tal cual si es liviano; si no,
/// reducida y en JPEG.
fn full_image(id: &str) -> Option<(String, Vec<u8>)> {
    let path = crate::clipboard_history::image_path(id)?;
    let bytes = std::fs::read(&path).ok()?;
    let img = image::load_from_memory(&bytes).ok()?;
    if bytes.len() <= atic_sync::MAX_IMAGE_BYTES && img.width().max(img.height()) <= FULL_MAX_PX {
        return Some(("image/png".into(), bytes));
    }
    let resized = img.thumbnail(FULL_MAX_PX, FULL_MAX_PX).to_rgb8();
    let mut out = std::io::Cursor::new(Vec::new());
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 88)
        .encode_image(&resized)
        .ok()?;
    Some(("image/jpeg".into(), out.into_inner()))
}

/// Una imagen del celular al portapapeles del PC. El historial la recoge solo.
fn paste_phone_image(data: &[u8]) -> Result<(), String> {
    // `copy_png_to_clipboard` espera RGBA de 8 bits; un JPEG o un PNG sin alfa
    // del celular lo rompería («el PNG no tiene el tamaño esperado»).
    let img = image::DynamicImage::ImageRgba8(image::load_from_memory(data).map_err(|e| e.to_string())?.to_rgba8());
    let path = std::env::temp_dir().join(format!("atic-phone-{}.png", uuid::Uuid::new_v4()));
    img.save_with_format(&path, image::ImageFormat::Png).map_err(|e| e.to_string())?;
    let result = crate::capture::copy_png_to_clipboard(&path);
    let _ = std::fs::remove_file(&path);
    result
}

/// Lo que suena y si se está grabando, en la forma del protocolo.
async fn pc_state(app: &AppHandle, media_key: &mut Option<String>) -> PcState {
    // `known` evita traer la carátula (cientos de KB) si no cambió: el celular no la usa.
    let media = crate::media::media_now(media_key.clone()).await.ok().flatten();
    *media_key = media.as_ref().map(|m| m.thumb_key.clone());
    let recording = app
        .state::<AppState>()
        .active
        .lock_or_recover()
        .as_ref()
        .map(|active| RecordingState { started_at_ms: active.recording.started_at.timestamp_millis() });
    PcState {
        media: media.filter(|m| !m.title.trim().is_empty()).map(|m| MediaState {
            title: m.title,
            artist: m.artist,
            app: m.app,
            playing: m.playing,
            can_toggle: m.can_toggle,
            can_next: m.can_next,
            can_prev: m.can_prev,
        }),
        recording,
    }
}

async fn publish_loop(app: AppHandle, desktop: Arc<Desktop>) {
    let mut last: Option<Vec<AgentCard>> = None;
    let mut last_check = Instant::now();
    let mut last_pc = Instant::now() - PC_REFRESH;
    let mut media_key = None;
    loop {
        tokio::time::sleep(TICK).await;
        if PC_DIRTY.swap(false, Ordering::Relaxed) || last_pc.elapsed() >= PC_REFRESH {
            last_pc = Instant::now();
            desktop.publish_pc(pc_state(&app, &mut media_key).await);
            desktop.publish_clip_history(clip_history());
        }
        let due = DIRTY.swap(false, Ordering::Relaxed) || last_check.elapsed() >= FULL_REFRESH;
        if !due {
            continue;
        }
        last_check = Instant::now();
        let cards = snapshot();
        if last.as_ref() != Some(&cards) {
            desktop.publish(cards.clone());
            last = Some(cards);
        }
    }
}

fn snapshot() -> Vec<AgentCard> {
    let mut cards: Vec<AgentCard> = agents::bridge::phone_sessions()
        .into_iter()
        .map(|s| {
            let permission = s.pending.first().map(|p| PermissionAsk {
                id: p.id.clone(),
                title: permission_title(&p.tool),
                detail: permission_detail(&p.description, &p.input),
                // Solo Claude Code guarda la regla sugerida; el resto la trata como «permitir».
                can_allow_always: s.backend == "claude-code",
            });
            let status = if permission.is_some() {
                AgentStatus::Waiting
            } else if s.running {
                AgentStatus::Working
            } else {
                AgentStatus::Ready
            };
            AgentCard {
                project: s.label.unwrap_or_else(|| project_name(&s.cwd)),
                id: s.id,
                backend_id: s.backend,
                backend_name: s.backend_name,
                status,
                activity: None,
                preview: None,
                permission,
            }
        })
        .collect();

    if agents::PAGER_ENABLED {
        cards.extend(presence::snapshot().into_iter().map(|p| AgentCard {
            project: project_name(&p.cwd),
            id: p.id,
            backend_id: p.backend_id,
            backend_name: p.backend_name,
            status: match p.status {
                presence::PresenceStatus::Working => AgentStatus::Working,
                presence::PresenceStatus::Waiting => AgentStatus::Waiting,
                presence::PresenceStatus::Ready => AgentStatus::Ready,
                presence::PresenceStatus::Idle => AgentStatus::Idle,
            },
            activity: p.activity.map(|a| AgentActivity {
                kind: match a.kind {
                    presence::ActivityKind::Thinking => ActivityKind::Thinking,
                    presence::ActivityKind::Writing => ActivityKind::Writing,
                    presence::ActivityKind::Editing => ActivityKind::Editing,
                    presence::ActivityKind::Reading => ActivityKind::Reading,
                    presence::ActivityKind::Searching => ActivityKind::Searching,
                    presence::ActivityKind::Running => ActivityKind::Running,
                    presence::ActivityKind::Delegating => ActivityKind::Delegating,
                    presence::ActivityKind::Tool => ActivityKind::Tool,
                },
                detail: a.detail,
            }),
            preview: p.preview,
            permission: None,
        }));
    }
    cards
}

/// Último segmento del `cwd`: el nombre que el usuario reconoce.
fn project_name(cwd: &str) -> String {
    cwd.trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or(cwd)
        .to_string()
}

fn permission_title(tool: &str) -> String {
    match tool {
        "Bash" | "bash" | "shell" | "exec" => "Ejecutar comando".into(),
        "Edit" | "MultiEdit" | "Write" | "edit" | "write" | "apply_patch" => "Editar archivo".into(),
        "WebFetch" | "WebSearch" => "Usar la web".into(),
        other => format!("Usar {other}"),
    }
}

/// Lo que de verdad se aprueba: el comando o el archivo si el input lo trae;
/// si no, la descripción del agente.
fn permission_detail(description: &str, input: &str) -> Option<String> {
    let from_input = serde_json::from_str::<serde_json::Value>(input).ok().and_then(|v| {
        ["command", "file_path", "path", "url"]
            .iter()
            .find_map(|k| v.get(k).and_then(|x| x.as_str()).map(str::to_string))
    });
    let text = from_input.unwrap_or_else(|| description.trim().to_string());
    if text.is_empty() {
        return None;
    }
    let mut short: String = text.chars().take(200).collect();
    if short.len() < text.len() {
        short.push('…');
    }
    Some(short)
}

fn status(app: &AppHandle) -> PhoneStatus {
    let guard = RUNNING.lock_or_recover();
    let connected = guard.as_ref().map(|r| r.connected.clone()).unwrap_or_default();
    let devices = load_devices(app);
    PhoneStatus {
        running: guard.is_some(),
        clipboard: devices.clipboard,
        pending_pair: guard.as_ref().and_then(|r| r.pending.clone()),
        devices: devices
            .devices
            .into_iter()
            .map(|d| DeviceView { connected: connected.contains(&d.id), id: d.id, name: d.name, paired_at: d.paired_at })
            .collect(),
    }
}

/// Trae la ventana principal: el pedido de pareo no se puede contestar si no se ve.
fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

fn emit_status(app: &AppHandle) {
    let _ = app.emit("phone-sync", status(app));
}

fn devices_path(app: &AppHandle) -> PathBuf {
    app.state::<AppState>().dirs.config_path().with_file_name("phone.json")
}

fn load_devices(app: &AppHandle) -> Devices {
    let devices: Devices = std::fs::read_to_string(devices_path(app))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    CLIPBOARD_ON.store(devices.clipboard, Ordering::Relaxed);
    devices
}

fn save_devices(app: &AppHandle, devices: &Devices) -> Result<(), String> {
    let json = serde_json::to_string_pretty(devices).map_err(|e| e.to_string())?;
    atic_core::fs_atomic::write_atomic_str(&devices_path(app), &json).map_err(|e| e.to_string())
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
        .or_else(|_| std::env::var("HOSTNAME"))
        .map(|n| format!("Atic en {n}"))
        .unwrap_or_else(|_| "Atic".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_proyecto_es_el_ultimo_segmento_del_cwd() {
        assert_eq!(project_name(r"C:\Users\Lenovo\Documents\atic"), "atic");
        assert_eq!(project_name("/home/u/proyecto/"), "proyecto");
        assert_eq!(project_name("solo"), "solo");
    }

    #[test]
    fn el_detalle_prefiere_el_comando_a_la_descripcion() {
        let input = r#"{"command":"cargo test -p atic-sync","description":"tests"}"#;
        assert_eq!(permission_detail("Correr tests", input).as_deref(), Some("cargo test -p atic-sync"));
        // Input recortado (JSON roto): cae a la descripción.
        assert_eq!(permission_detail("Correr tests", r#"{"command":"cargo te"#).as_deref(), Some("Correr tests"));
        assert_eq!(permission_detail("  ", "{}"), None);
    }

    #[test]
    fn el_detalle_largo_se_corta_con_puntos() {
        let long = "x".repeat(300);
        let d = permission_detail(&long, "").unwrap();
        assert_eq!(d.chars().count(), 201);
        assert!(d.ends_with('…'));
    }

    #[test]
    fn la_clave_va_y_vuelve_en_hex() {
        let key = atic_sync::generate_secret_key();
        let hex: String = key.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(decode_hex(&hex), Some(key));
        assert_eq!(decode_hex("zz"), None);
    }
}
