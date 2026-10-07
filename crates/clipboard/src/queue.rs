//! La cola de «pegar después» (`paste_queue.json`): lo dictado cuando no había
//! una app externa donde pegarlo. Se pega en orden en cuanto otra app toma el
//! foco.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::now_ms;

pub const MAX_QUEUED: usize = 20;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PasteQueueItem {
    pub id: String,
    pub text: String,
    pub created_at_ms: u64,
}

impl PasteQueueItem {
    /// `None` si el texto queda vacío al recortarlo.
    pub fn new(text: &str) -> Option<Self> {
        let trimmed = text.trim();
        (!trimmed.is_empty()).then(|| Self {
            id: uuid::Uuid::new_v4().to_string(),
            text: trimmed.to_string(),
            created_at_ms: now_ms(),
        })
    }
}

/// La cola en memoria, guardada en `path` con cada cambio.
pub struct PasteQueue {
    path: PathBuf,
    pub items: Vec<PasteQueueItem>,
}

impl PasteQueue {
    pub fn load(path: &Path) -> Self {
        let items = std::fs::read_to_string(path)
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default();
        Self { path: path.to_path_buf(), items }
    }

    pub fn save(&self) {
        if let Ok(raw) = serde_json::to_string_pretty(&self.items) {
            let _ = atic_core::write_atomic_str(&self.path, &raw);
        }
    }

    /// Suma al final; si pasa del tope se pierde lo más viejo.
    pub fn push(&mut self, item: PasteQueueItem) {
        self.items.push(item);
        while self.items.len() > MAX_QUEUED {
            self.items.remove(0);
        }
        self.save();
    }

    /// Saca el primero si sigue siendo `id` (pudo cambiar mientras se pegaba).
    pub fn remove_front(&mut self, id: &str) -> bool {
        if self.items.first().is_some_and(|item| item.id == id) {
            self.items.remove(0);
            self.save();
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guarda_en_orden_con_tope_y_saca_solo_el_frente() {
        let dir = std::env::temp_dir().join(format!("atic-queue-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("paste_queue.json");
        let _ = std::fs::remove_file(&path);

        let mut queue = PasteQueue::load(&path);
        assert!(PasteQueueItem::new("   ").is_none());
        for n in 0..MAX_QUEUED + 2 {
            queue.push(PasteQueueItem::new(&format!("texto {n}")).unwrap());
        }
        assert_eq!(queue.items.len(), MAX_QUEUED);
        assert_eq!(queue.items[0].text, "texto 2");

        let second = queue.items[1].id.clone();
        assert!(!queue.remove_front(&second));
        let first = queue.items[0].id.clone();
        assert!(queue.remove_front(&first));
        assert_eq!(PasteQueue::load(&path).items.len(), MAX_QUEUED - 1);
        let _ = std::fs::remove_dir_all(dir);
    }
}
