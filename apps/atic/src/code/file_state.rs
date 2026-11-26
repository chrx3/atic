//! Lo abierto del árbol de Archivos en cada espacio y si se muestran los
//! ignorados: se guarda en esta máquina, en `code-files.json`, junto a
//! `code-marks.json` (patrón `Marks::load/save` de `marks.rs`).

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

const FILE: &str = "code-files.json";

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
pub struct FileState {
    /// Por espacio: las carpetas que quedaron abiertas (rutas completas).
    open: HashMap<String, Vec<String>>,
    /// El interruptor «Mostrar ignorados» del panel Archivos.
    show_ignored: bool,
    /// No se guarda en disco (para las pruebas).
    #[serde(skip)]
    memory_only: bool,
}

impl FileState {
    pub fn load() -> Self {
        crate::paths::file(FILE)
            .and_then(|path| std::fs::read_to_string(path).ok())
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    #[cfg(test)]
    fn in_memory() -> Self {
        Self { memory_only: true, ..Default::default() }
    }

    fn save(&self) {
        if self.memory_only {
            return;
        }
        let Some(path) = crate::paths::file(FILE) else {
            return;
        };
        match serde_json::to_string_pretty(self) {
            Ok(text) => {
                if let Err(error) = std::fs::write(&path, text) {
                    tracing::warn!(%error, "atic code: no se guardó el estado del árbol");
                }
            }
            Err(error) => tracing::warn!(%error, "atic code: estado del árbol sin serializar"),
        }
    }

    /// Olvida las carpetas abiertas de un espacio eliminado.
    pub fn forget(&mut self, id: u64) {
        if self.open.remove(&id.to_string()).is_some() {
            self.save();
        }
    }

    /// Las carpetas abiertas del espacio `id`.
    pub fn open_for(&self, id: u64) -> Vec<PathBuf> {
        self.open.get(&id.to_string()).map(|paths| paths.iter().map(PathBuf::from).collect()).unwrap_or_default()
    }

    /// Guarda las carpetas abiertas del espacio `id`.
    pub fn set_open(&mut self, id: u64, paths: Vec<PathBuf>) {
        let key = id.to_string();
        let paths: Vec<String> = paths.iter().map(|p| p.display().to_string()).collect();
        if paths.is_empty() {
            self.open.remove(&key);
        } else {
            self.open.insert(key, paths);
        }
        self.save();
    }

    pub fn show_ignored(&self) -> bool {
        self.show_ignored
    }

    pub fn set_show_ignored(&mut self, show: bool) {
        if self.show_ignored != show {
            self.show_ignored = show;
            self.save();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lo_abierto_y_el_interruptor_sobreviven_a_releer() {
        let mut state = FileState::in_memory();
        assert!(state.open_for(7).is_empty());
        assert!(!state.show_ignored());
        state.set_open(7, vec![PathBuf::from("/tmp/e/src"), PathBuf::from("/tmp/e/web")]);
        state.set_show_ignored(true);
        let text = serde_json::to_string(&state).unwrap();
        let back: FileState = serde_json::from_str(&text).unwrap();
        assert_eq!(back.open_for(7), vec![PathBuf::from("/tmp/e/src"), PathBuf::from("/tmp/e/web")]);
        assert!(back.show_ignored());
        assert!(back.open_for(9).is_empty());
        // Un archivo de antes, sin los campos, se lee igual.
        let old: FileState = serde_json::from_str(r#"{}"#).unwrap();
        assert!(!old.show_ignored());
    }
}
