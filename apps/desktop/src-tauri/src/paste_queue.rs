//! Cola de pegado cuando no hay ventana externa con foco (`pegar después`).

use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::clipboard_history;
use crate::state::AppState;

const MAX_ITEMS: usize = 20;
const POLL_MS: u64 = 400;
const FOCUS_SETTLE_MS: u64 = 220;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PasteQueueItem {
    pub id: String,
    pub text: String,
    pub created_at_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PasteOutcome {
    Pasted,
    Queued,
}

struct QueueState {
    items: Vec<PasteQueueItem>,
}

static QUEUE: Mutex<Option<QueueState>> = Mutex::new(None);
static POLLER: OnceLock<()> = OnceLock::new();

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn load_from_disk(path: &std::path::Path) -> Vec<PasteQueueItem> {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    serde_json::from_str(&raw).unwrap_or_default()
}

fn save_to_disk(path: &std::path::Path, items: &[PasteQueueItem]) {
    if let Ok(raw) = serde_json::to_string_pretty(items) {
        let _ = std::fs::write(path, raw);
    }
}

fn ensure_queue_loaded(state: &AppState) {
    let path = state.dirs.paste_queue_path();
    let mut guard = QUEUE.lock().unwrap();
    if guard.is_none() {
        *guard = Some(QueueState {
            items: load_from_disk(&path),
        });
    }
}

fn with_queue<R>(state: &AppState, f: impl FnOnce(&[PasteQueueItem]) -> R) -> R {
    ensure_queue_loaded(state);
    let guard = QUEUE.lock().unwrap();
    f(&guard.as_ref().unwrap().items)
}

fn with_queue_mut<R>(state: &AppState, f: impl FnOnce(&mut Vec<PasteQueueItem>) -> R) -> R {
    let path = state.dirs.paste_queue_path();
    ensure_queue_loaded(state);
    let mut guard = QUEUE.lock().unwrap();
    let items = &mut guard.as_mut().unwrap().items;
    let result = f(items);
    save_to_disk(&path, items);
    result
}

fn emit_changed(app: &AppHandle) {
    let _ = app.emit("paste-queue-changed", ());
}

fn emit_queued(app: &AppHandle, preview: &str) {
    let _ = app.emit(
        "paste-queued",
        serde_json::json!({
            "preview": preview.chars().take(120).collect::<String>(),
        }),
    );
}

/// Oculta la pill, restaura el foco externo y espera a que el SO asiente.
fn prepare_external_paste(app: &AppHandle) -> Option<tauri::WebviewWindow> {
    let pill = app.get_webview_window("pill");
    if let Some(ref win) = pill {
        let _ = win.hide();
    }
    clipboard_history::focus_paste_target();
    thread::sleep(Duration::from_millis(FOCUS_SETTLE_MS));
    pill
}

fn restore_pill(pill: Option<tauri::WebviewWindow>) {
    if let Some(win) = pill {
        // La espera y el "sin activar" son por lo mismo: `SendInput` encola las
        // teclas y no espera a que la app destino las procese. Reaparecer al
        // frente en ese hueco se comía el pegado.
        thread::sleep(Duration::from_millis(120));
        clipboard_history::show_without_stealing_focus(&win);
    }
}

fn ensure_poller(app: &AppHandle) {
    POLLER.get_or_init(|| {
        let handle = app.clone();
        thread::spawn(move || {
            loop {
                thread::sleep(Duration::from_millis(POLL_MS));
                let has_items = {
                    let guard = QUEUE.lock().unwrap();
                    guard.as_ref().map(|q| !q.items.is_empty()).unwrap_or(false)
                };
                if !has_items {
                    continue;
                }
                // Solo auto-pegar cuando una app externa YA tiene el foco.
                // Un HWND guardado no basta: SendInput va al primer plano.
                if clipboard_history::has_live_external_foreground() {
                    let _ = flush_front_internal(&handle);
                }
            }
        });
    });
}

/// Encola texto y notifica al frontend.
pub(crate) fn enqueue(app: &AppHandle, text: &str) -> Result<PasteQueueItem, String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err("Texto vacío".into());
    }
    let state = app.state::<AppState>();
    let preview: String = trimmed.chars().take(120).collect();
    let item = PasteQueueItem {
        id: uuid::Uuid::new_v4().to_string(),
        text: trimmed.to_string(),
        created_at_ms: now_ms(),
    };
    with_queue_mut(&state, |items| {
        items.push(item.clone());
        while items.len() > MAX_ITEMS {
            items.remove(0);
        }
    });
    emit_changed(app);
    emit_queued(app, &preview);
    ensure_poller(app);
    Ok(item)
}

/// Pega si hay destino externo listo; si no, agentes (si está a la vista); si
/// no, encola.
///
/// El orden importa: con la burbuja de agentes abierta, el dictado iba siempre
/// al compositor de agentes aunque el usuario estuviera en Chrome/Word/etc.
/// Primero el input externo donde estaba (o el que tocó mientras hablaba);
/// agentes solo si no hay destino externo.
pub(crate) fn try_paste_or_enqueue(app: &AppHandle, text: &str) -> Result<PasteOutcome, String> {
    // Preferir primer plano vivo (tras focus_paste_target del caller).
    if clipboard_history::has_live_external_foreground() {
        tracing::debug!(target: "paste_geo", "TRY        primer plano externo vivo -> pego");
        clipboard_history::paste_text(app, text)?;
        return Ok(PasteOutcome::Pasted);
    }
    // Hay HWND guardado pero Atic sigue con foco: intentar restaurar una vez.
    if clipboard_history::has_saved_external_paste_target() {
        tracing::debug!(
            target: "paste_geo",
            "TRY        Atic tiene el foco; reintento traer el destino guardado"
        );
        clipboard_history::focus_paste_target();
        thread::sleep(Duration::from_millis(FOCUS_SETTLE_MS));
        if clipboard_history::has_live_external_foreground() {
            tracing::debug!(target: "paste_geo", "TRY        recuperado -> pego");
            clipboard_history::paste_text(app, text)?;
            return Ok(PasteOutcome::Pasted);
        }
        tracing::debug!(
            target: "paste_geo",
            "TRY        no se pudo traer el destino al frente"
        );
    }
    // Sin destino externo usable: si agentes está a la vista, insertar ahí
    // (evento interno; Ctrl+V externo sacaría el foco y taparía la burbuja).
    if clipboard_history::agents_visible(app) {
        tracing::debug!(target: "paste_geo", "TRY        sin externo; agentes visible -> insert interno");
        clipboard_history::insert_text_into_agents(app, text)?;
        return Ok(PasteOutcome::Pasted);
    }
    if clipboard_history::has_saved_external_paste_target() {
        tracing::debug!(
            target: "paste_geo",
            "TRY        no se pudo traer el destino al frente -> encolo"
        );
    } else {
        tracing::debug!(target: "paste_geo", "TRY        sin destino externo guardado -> encolo");
    }
    enqueue(app, text)?;
    Ok(PasteOutcome::Queued)
}

/// Igual que [`try_paste_or_enqueue`] pero siempre devuelve `Ok(())`.
pub(crate) fn paste_text_or_enqueue(app: &AppHandle, text: &str) -> Result<(), String> {
    try_paste_or_enqueue(app, text)?;
    Ok(())
}

/// Pega el frente sin borrarlo hasta confirmar éxito.
///
/// El poller solo llama esto con primer plano externo ya activo (SendInput
/// llega ahí). No oculta la pill ni restaura HWND guardado.
fn flush_front_internal(app: &AppHandle) -> Result<bool, String> {
    let state = app.state::<AppState>();
    let front = with_queue(&state, |items| items.first().cloned());
    let Some(item) = front else {
        return Ok(false);
    };

    clipboard_history::paste_text(app, &item.text)?;

    let removed = with_queue_mut(&state, |items| {
        if items.first().is_some_and(|i| i.id == item.id) {
            items.remove(0);
            true
        } else {
            // El frente cambió mientras pegábamos; no tocar la cola.
            false
        }
    });
    if removed {
        emit_changed(app);
    }
    Ok(true)
}

#[tauri::command]
pub fn list_paste_queue(state: State<AppState>) -> Result<Vec<PasteQueueItem>, String> {
    let items = with_queue(&state, |items| items.to_vec());
    Ok(items)
}

#[tauri::command]
pub fn enqueue_paste(
    app: AppHandle,
    state: State<AppState>,
    text: String,
) -> Result<PasteQueueItem, String> {
    let _ = &state;
    enqueue(&app, &text)
}

#[tauri::command]
pub fn dismiss_paste_queue_item(
    app: AppHandle,
    state: State<AppState>,
    id: String,
) -> Result<(), String> {
    let removed = with_queue_mut(&state, |items| {
        let idx = items.iter().position(|i| i.id == id)?;
        Some(items.remove(idx))
    });
    if removed.is_none() {
        return Err("Ítem no encontrado".into());
    }
    emit_changed(&app);
    Ok(())
}

#[tauri::command]
pub fn clear_paste_queue(app: AppHandle, state: State<AppState>) -> Result<(), String> {
    with_queue_mut(&state, |items| items.clear());
    emit_changed(&app);
    Ok(())
}

#[tauri::command]
pub fn paste_queue_item_now(
    app: AppHandle,
    state: State<AppState>,
    id: String,
) -> Result<(), String> {
    let text = with_queue(&state, |items| {
        items.iter().find(|i| i.id == id).map(|i| i.text.clone())
    })
    .ok_or_else(|| "Ítem no encontrado".to_string())?;

    let pill = prepare_external_paste(&app);
    let paste_result = clipboard_history::paste_text(&app, &text);
    restore_pill(pill);
    paste_result?;

    let removed = with_queue_mut(&state, |items| {
        let idx = items.iter().position(|i| i.id == id)?;
        Some(items.remove(idx))
    });
    if removed.is_none() {
        // Pegado OK pero el ítem ya no estaba (p. ej. dismiss concurrente).
        emit_changed(&app);
        return Ok(());
    }
    emit_changed(&app);
    Ok(())
}

/// Intenta pegar el ítem más antiguo si hay HWND externo en primer plano.
#[tauri::command]
pub fn paste_queue_flush_ready(app: AppHandle) -> Result<bool, String> {
    if !clipboard_history::has_live_external_foreground() {
        return Ok(false);
    }
    flush_front_internal(&app)
}
