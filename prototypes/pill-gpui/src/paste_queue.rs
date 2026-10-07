//! «Pegar después», como en Atic (`paste_queue.rs`): lo dictado sin una app
//! externa donde pegarlo queda en `paste_queue.json` y se pega en orden en
//! cuanto otra app toma el foco. `SendInput` va siempre al primer plano: un
//! destino guardado no basta.

use std::sync::{Mutex, Once};
use std::time::Duration;

use atic_clipboard::{PasteQueue, PasteQueueItem};
use atic_core::MutexExt;

use crate::paste;

const POLL: Duration = Duration::from_millis(400);
/// Lo que tarda el portapapeles en estar listo para el Ctrl+V.
const KEY_DELAY: Duration = Duration::from_millis(80);

static QUEUE: Mutex<Option<PasteQueue>> = Mutex::new(None);
static POLLER: Once = Once::new();

fn with_queue<R>(f: impl FnOnce(&mut PasteQueue) -> R) -> Option<R> {
    let mut guard = QUEUE.lock_or_recover();
    if guard.is_none() {
        let path = atic_core::AppDirs::new().ok()?.paste_queue_path();
        *guard = Some(PasteQueue::load(&path));
    }
    guard.as_mut().map(f)
}

/// Retoma lo que quedó en cola de la vez anterior.
pub fn start() {
    if with_queue(|queue| !queue.items.is_empty()).unwrap_or(false) {
        ensure_poller();
    }
}

/// Deja `text` para pegarlo cuando haya una app externa con el foco.
pub fn enqueue(text: &str) {
    let Some(item) = PasteQueueItem::new(text) else {
        return;
    };
    with_queue(|queue| queue.push(item));
    ensure_poller();
}

fn ensure_poller() {
    POLLER.call_once(|| {
        let spawned = std::thread::Builder::new()
            .name("pegar-despues".into())
            .spawn(|| loop {
                std::thread::sleep(POLL);
                if let Some(target) = paste::foreground_target() {
                    flush_front(target);
                }
            });
        if let Err(error) = spawned {
            tracing::warn!(%error, "pegar después: no arrancó el hilo");
        }
    });
}

/// Pega el primero y recién entonces lo saca de la cola.
fn flush_front(target: paste::Target) {
    let Some(item) = with_queue(|queue| queue.items.first().cloned()).flatten() else {
        return;
    };
    if let Err(error) = atic_clipboard::set_system_text(item.text) {
        tracing::debug!(%error, "pegar después: no se pudo escribir el portapapeles");
        return;
    }
    std::thread::sleep(KEY_DELAY);
    paste::send_paste_chord(paste::needs_ctrl_shift_v(target));
    with_queue(|queue| queue.remove_front(&item.id));
}
