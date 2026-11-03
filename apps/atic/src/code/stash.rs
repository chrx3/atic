//! Borradores guardados del compositor: lo que se estaba escribiendo (texto, archivos,
//! pegados largos e imágenes) se aparta para escribir otra cosa y se recupera después,
//! desde el menú de acciones. Valen para todas las conversaciones y quedan en disco.

use serde::{Deserialize, Serialize};

/// Cuántos borradores se guardan: al pasarse, se descarta el más viejo.
const MAX_ENTRIES: usize = 12;

/// Una imagen adjunta, tal como se envía (base64) para poder volver a mostrarla.
#[derive(Clone, Serialize, Deserialize)]
pub struct StashImage {
    pub name: String,
    pub media_type: String,
    pub data: String,
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct StashEntry {
    pub id: u64,
    pub text: String,
    #[serde(default)]
    pub files: Vec<String>,
    #[serde(default)]
    pub pasted: Vec<String>,
    #[serde(default)]
    pub images: Vec<StashImage>,
}

impl StashEntry {
    pub fn has_attachments(&self) -> bool {
        !(self.files.is_empty() && self.pasted.is_empty() && self.images.is_empty())
    }

    /// Lo que se escribió, en una línea, para reconocer el borrador en el menú.
    pub fn snippet(&self) -> String {
        let line = self.text.lines().find(|l| !l.trim().is_empty()).unwrap_or_default().trim();
        let mut label: String = line.chars().take(70).collect();
        if line.chars().count() > 70 {
            label.push('…');
        }
        label
    }
}

#[derive(Default, Serialize, Deserialize)]
pub struct Stash {
    /// El más nuevo primero.
    entries: Vec<StashEntry>,
    next_id: u64,
}

impl Stash {
    pub fn load() -> Self {
        crate::paths::file("code-stash.json")
            .and_then(|path| std::fs::read_to_string(path).ok())
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        // Una ventana abierta para capturas no guarda nada.
        if std::env::var_os("CODE_OPEN").is_some() {
            return;
        }
        let Some(path) = crate::paths::file("code-stash.json") else {
            return;
        };
        match serde_json::to_string(self) {
            Ok(text) => {
                if let Err(error) = std::fs::write(&path, text) {
                    tracing::warn!(%error, "atic code: no se guardaron los borradores");
                }
            }
            Err(error) => tracing::warn!(%error, "atic code: borradores sin serializar"),
        }
    }

    pub fn entries(&self) -> &[StashEntry] {
        &self.entries
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Guarda un borrador (su `id` lo pone la pila) y descarta el más viejo si ya son demasiados.
    pub fn push(&mut self, mut entry: StashEntry) -> u64 {
        self.next_id += 1;
        entry.id = self.next_id;
        self.entries.insert(0, entry);
        self.entries.truncate(MAX_ENTRIES);
        self.next_id
    }

    /// Saca el borrador `id` de la pila.
    pub fn take(&mut self, id: u64) -> Option<StashEntry> {
        let index = self.entries.iter().position(|e| e.id == id)?;
        Some(self.entries.remove(index))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(text: &str) -> StashEntry {
        StashEntry { text: text.into(), ..StashEntry::default() }
    }

    #[test]
    fn el_mas_nuevo_va_primero_y_take_lo_saca() {
        let mut stash = Stash::default();
        let uno = stash.push(entry("uno"));
        let dos = stash.push(entry("dos"));
        assert_ne!(uno, dos);
        assert_eq!(stash.entries()[0].text, "dos");
        assert_eq!(stash.take(uno).map(|e| e.text), Some("uno".into()));
        assert!(stash.take(uno).is_none());
        assert_eq!(stash.len(), 1);
    }

    #[test]
    fn al_pasarse_del_maximo_se_descarta_el_mas_viejo() {
        let mut stash = Stash::default();
        let first = stash.push(entry("primero"));
        for n in 0..MAX_ENTRIES {
            stash.push(entry(&format!("borrador {n}")));
        }
        assert_eq!(stash.len(), MAX_ENTRIES);
        assert!(stash.take(first).is_none());
    }

    #[test]
    fn el_resumen_toma_la_primera_linea_con_texto_y_la_acorta() {
        assert_eq!(entry("\n\n  hola mundo  \nsegunda").snippet(), "hola mundo");
        let largo = "x".repeat(100);
        let resumen = entry(&largo).snippet();
        assert_eq!(resumen.chars().count(), 71);
        assert!(resumen.ends_with('…'));
        assert_eq!(entry("").snippet(), "");
    }

    #[test]
    fn guarda_y_lee_los_adjuntos() {
        let mut stash = Stash::default();
        stash.push(StashEntry { files: vec!["a.rs".into()], ..StashEntry::default() });
        let json = serde_json::to_string(&stash).unwrap();
        let back: Stash = serde_json::from_str(&json).unwrap();
        assert!(back.entries()[0].has_attachments());
        assert!(!entry("solo texto").has_attachments());
    }
}
