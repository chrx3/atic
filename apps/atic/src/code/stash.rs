//! Borradores guardados del compositor: lo que se estaba escribiendo (texto, archivos,
//! pegados largos e imágenes) se aparta para escribir otra cosa y se recupera después,
//! desde el menú de acciones. Valen para todas las conversaciones y quedan en disco.

use serde::{Deserialize, Serialize};

/// Cuántos borradores se guardan: al pasarse, se descarta el más viejo.
pub const MAX_ENTRIES: usize = 12;

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
    pub quotes: Vec<String>,
    #[serde(default)]
    pub images: Vec<StashImage>,
    /// Cuándo se apartó, en segundos Unix. Los borradores viejos (de antes de este
    /// campo) traen 0 y no muestran hora.
    #[serde(default)]
    pub saved_at: i64,
}

impl StashEntry {
    /// Lo que se escribió, en una línea, para reconocer el borrador en el menú.
    pub fn snippet(&self) -> String {
        Self::clean_line(self.text.lines().find(|l| !l.trim().is_empty()).unwrap_or_default())
    }

    /// La segunda línea con texto, para la tarjeta del menú (las dos primeras líneas
    /// distinguen borradores que empiezan igual). Vacía si no hay segunda línea.
    pub fn second_line(&self) -> String {
        Self::clean_line(self.text.lines().filter(|l| !l.trim().is_empty()).nth(1).unwrap_or_default())
    }

    fn clean_line(line: &str) -> String {
        let line = line.trim();
        let mut label: String = line.chars().take(70).collect();
        if line.chars().count() > 70 {
            label.push('…');
        }
        label
    }

    /// Segundos desde que se apartó (`None` en borradores viejos sin fecha).
    pub fn age_secs(&self, now: i64) -> Option<i64> {
        (self.saved_at > 0).then(|| (now - self.saved_at).max(0))
    }
}

#[derive(Default, Serialize, Deserialize)]
pub struct Stash {
    /// El más nuevo primero.
    entries: Vec<StashEntry>,
    next_id: u64,
    /// Lo último descartado, para deshacer (no se guarda en disco).
    #[serde(skip)]
    undone: Option<StashEntry>,
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
    /// Anota cuándo se apartó para la hora relativa del menú.
    pub fn push(&mut self, mut entry: StashEntry) -> u64 {
        self.next_id += 1;
        entry.id = self.next_id;
        if entry.saved_at <= 0 {
            entry.saved_at = chrono::Utc::now().timestamp();
        }
        self.entries.insert(0, entry);
        self.entries.truncate(MAX_ENTRIES);
        self.next_id
    }

    /// Saca el borrador `id` de la pila.
    pub fn take(&mut self, id: u64) -> Option<StashEntry> {
        let index = self.entries.iter().position(|e| e.id == id)?;
        Some(self.entries.remove(index))
    }

    /// Saca el borrador `id` acordándose de él para deshacer.
    pub fn drop_with_undo(&mut self, id: u64) -> bool {
        match self.take(id) {
            Some(entry) => {
                self.undone = Some(entry);
                true
            }
            None => false,
        }
    }

    /// Si hay algo descartado por recuperar.
    pub fn has_undo(&self) -> bool {
        self.undone.is_some()
    }

    /// Devuelve lo último descartado al frente de la pila.
    pub fn restore_undo(&mut self) -> bool {
        match self.undone.take() {
            Some(entry) => {
                self.entries.insert(0, entry);
                self.entries.truncate(MAX_ENTRIES);
                true
            }
            None => false,
        }
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
    fn la_segunda_linea_distigue_borradores_que_empiezan_igual() {
        assert_eq!(entry("título\n\nsegunda línea").second_line(), "segunda línea");
        assert_eq!(entry("solo una").second_line(), "");
        assert_eq!(entry("").second_line(), "");
    }

    #[test]
    fn al_guardar_se_anota_cuando_y_lo_viejo_no_tiene_hora() {
        let mut stash = Stash::default();
        stash.push(entry("nuevo"));
        let saved = stash.entries()[0].saved_at;
        assert!(saved > 0);
        assert_eq!(stash.entries()[0].age_secs(saved + 90), Some(90));
        // Un JSON de antes del campo fecha se lee igual y no muestra hora.
        let viejo: StashEntry = serde_json::from_str(r#"{"id":1,"text":"viejo"}"#).unwrap();
        assert_eq!(viejo.saved_at, 0);
        assert_eq!(viejo.age_secs(saved), None);
    }

    #[test]
    fn descartar_guarda_para_deshacer_y_lo_devuelve_al_frente() {
        let mut stash = Stash::default();
        let uno = stash.push(entry("uno"));
        stash.push(entry("dos"));
        assert!(!stash.has_undo());
        assert!(stash.drop_with_undo(uno));
        assert!(stash.has_undo());
        assert_eq!(stash.len(), 1);
        assert!(stash.restore_undo());
        assert!(!stash.has_undo());
        assert_eq!(stash.len(), 2);
        assert_eq!(stash.entries()[0].text, "uno");
        assert!(!stash.drop_with_undo(999));
        assert!(!stash.restore_undo());
    }

    #[test]
    fn guarda_y_lee_los_adjuntos() {
        let mut stash = Stash::default();
        stash.push(StashEntry { files: vec!["a.rs".into()], ..StashEntry::default() });
        let json = serde_json::to_string(&stash).unwrap();
        let back: Stash = serde_json::from_str(&json).unwrap();
        assert_eq!(back.entries()[0].files, vec!["a.rs".to_string()]);
    }
}
