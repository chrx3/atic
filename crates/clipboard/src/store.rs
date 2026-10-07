use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::{ClipboardItem, HISTORY_FILE, MAX_ITEMS};

/// Una captura recién copiada al portapapeles, esperando que el watcher la vea.
///
/// No se puede resolver esto con `suppress_until`: la imagen se queda en el
/// portapapeles indefinidamente, así que una ventana de tiempo solo retrasa el
/// duplicado hasta que expira. Y tampoco sirve precalcular el fingerprint de
/// contenido: el round-trip por el DIB de Windows no garantiza los mismos
/// bytes. Lo que sí sabemos con certeza son las dimensiones.
pub struct PendingCapture {
    /// `capture:<id>` — la identidad que ya quedó en el historial.
    pub fingerprint: String,
    pub width: usize,
    pub height: usize,
    pub at: SystemTime,
}

/// Cuánto vale la pena esperar a que el watcher vea nuestra propia captura.
const PENDING_CAPTURE_TTL: Duration = Duration::from_secs(10);

impl PendingCapture {
    pub fn is_fresh(&self) -> bool {
        self.at
            .elapsed()
            .is_ok_and(|age| age < PENDING_CAPTURE_TTL)
    }

    /// ¿El watcher acaba de ver la imagen que nosotros mismos pusimos?
    pub fn matches(&self, width: usize, height: usize) -> bool {
        self.is_fresh() && self.width == width && self.height == height
    }
}

/// El historial en memoria, compartido entre el watcher y quien lo muestra.
#[derive(Default)]
pub struct History {
    pub items: Vec<ClipboardItem>,
    pub last_fingerprint: Option<String>,
    /// Evita re-capturar lo que nosotros mismos pegamos.
    pub suppress_until: Option<SystemTime>,
    /// Ítems borrados a mano: el SO puede seguir teniendo el mismo contenido
    /// en el portapapeles; sin esto el watcher los re-ingiere al instante.
    pub deleted_fingerprints: HashSet<String>,
    /// La captura que acabamos de poner en el portapapeles, para que el watcher
    /// la reconozca como tal en vez de grabar una copia paralela.
    pub pending_capture: Option<PendingCapture>,
}

impl History {
    /// El historial guardado en `dir`.
    pub fn load(dir: &Path) -> Self {
        let items = load_history(dir);
        let last = items.first().map(|i| i.fingerprint.clone());
        Self {
            items,
            last_fingerprint: last,
            suppress_until: None,
            // Las capturas descartadas sí sobreviven al reinicio: el PNG sigue en
            // la carpeta de capturas, así que sin esto el backfill las resucita
            // en cada arranque.
            deleted_fingerprints: load_dismissed_captures(dir),
            pending_capture: None,
        }
    }

    pub fn push_item(&mut self, dir: &Path, mut item: ClipboardItem) {
        if self.deleted_fingerprints.contains(&item.fingerprint) {
            return;
        }
        if self
            .last_fingerprint
            .as_ref()
            .is_some_and(|f| f == &item.fingerprint)
        {
            return;
        }
        // Contenido nuevo distinto al borrado: ya se puede volver a capturar
        // el mismo fingerprint si el usuario lo copia otra vez más adelante.
        //
        // Las capturas se salvan de la limpieza. El razonamiento de arriba es «el
        // usuario lo volvió a copiar a propósito», y para una captura eso no
        // aplica: nadie la vuelve a copiar, la relee el listado desde el
        // directorio. Sin esta excepción, borrar una captura duraba hasta la
        // siguiente copia de cualquier cosa y después reaparecía sola.
        self.deleted_fingerprints
            .retain(|f| f.starts_with("capture:"));
        if let Some(idx) = self
            .items
            .iter()
            .position(|existing| existing.fingerprint == item.fingerprint)
        {
            let mut existing = self.items.remove(idx);
            existing.created_at_ms = item.created_at_ms;
            existing.pinned = existing.pinned || item.pinned;
            self.last_fingerprint = Some(existing.fingerprint.clone());
            self.items.insert(0, existing);
            save_history(dir, &self.items);
            return;
        }

        self.last_fingerprint = Some(item.fingerprint.clone());
        if item.source.is_empty() {
            item.source = "watcher".into();
        }
        self.items.insert(0, item);
        self.prune(dir);
        save_history(dir, &self.items);
    }

    pub fn prune(&mut self, dir: &Path) {
        while self.items.len() > MAX_ITEMS {
            let remove_idx = self
                .items
                .iter()
                .enumerate()
                .rev()
                .find(|(_, item)| !item.pinned)
                .map(|(i, _)| i)
                .unwrap_or(self.items.len() - 1);
            let removed = self.items.remove(remove_idx);
            if let Some(path) = removed.image_path {
                let p = PathBuf::from(path);
                if p.starts_with(dir) {
                    let _ = std::fs::remove_file(p);
                }
            }
        }
    }

    /// Suma al historial un ítem que trae el celular (de otro PC o del propio
    /// celular) con su id, sin tocar el portapapeles del sistema. Va en su lugar
    /// por fecha, no arriba de todo. `false` si ya estaba (por id o por contenido).
    pub fn insert_imported(&mut self, dir: &Path, item: ClipboardItem) -> bool {
        if self.deleted_fingerprints.contains(&item.fingerprint)
            || self
                .items
                .iter()
                .any(|i| i.id == item.id || i.fingerprint == item.fingerprint)
        {
            return false;
        }
        let at = self
            .items
            .iter()
            .position(|i| i.created_at_ms < item.created_at_ms)
            .unwrap_or(self.items.len());
        self.items.insert(at, item);
        self.prune(dir);
        save_history(dir, &self.items);
        true
    }
}

fn history_path(dir: &Path) -> PathBuf {
    dir.join(HISTORY_FILE)
}

pub fn load_history(dir: &Path) -> Vec<ClipboardItem> {
    let path = history_path(dir);
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    serde_json::from_str(&raw).unwrap_or_default()
}

/// Atómica: el historial se reescribe entero con cada copia —cada 450 ms hay
/// una oportunidad de morir a mitad—, y `load_history` ante un JSON roto
/// devuelve una lista vacía, o sea que un truncado se vería como «se borró
/// todo» y no como un error.
pub fn save_history(dir: &Path, items: &[ClipboardItem]) {
    if let Ok(raw) = serde_json::to_string_pretty(items) {
        let _ = atic_core::write_atomic_str(&history_path(dir), &raw);
    }
}

fn dismissed_path(dir: &Path) -> PathBuf {
    dir.join("dismissed-captures.json")
}

/// Capturas que el usuario sacó del historial, entre arranques.
///
/// Va aparte de `history.json` porque no es una lista de ítems sino de
/// ausencias: el PNG sigue existiendo en la carpeta de capturas —es del gestor
/// de capturas, el clipboard no debe borrarlo— y el backfill lo encontraría de
/// nuevo en cada listado.
pub fn load_dismissed_captures(dir: &Path) -> HashSet<String> {
    std::fs::read_to_string(dismissed_path(dir))
        .ok()
        .and_then(|raw| serde_json::from_str::<HashSet<String>>(&raw).ok())
        .unwrap_or_default()
}

pub fn save_dismissed_captures(dir: &Path, fingerprints: &HashSet<String>) {
    let captures: HashSet<&String> = fingerprints
        .iter()
        .filter(|f| f.starts_with("capture:"))
        .collect();
    if let Ok(raw) = serde_json::to_string(&captures) {
        let _ = atic_core::write_atomic_str(&dismissed_path(dir), &raw);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{fingerprint_text, ClipboardKind};

    fn pending(width: usize, height: usize, age: Duration) -> PendingCapture {
        PendingCapture {
            fingerprint: "capture:test.png".into(),
            width,
            height,
            at: SystemTime::now() - age,
        }
    }

    #[test]
    fn pending_fresco_del_mismo_tamano_se_reconoce() {
        let p = pending(805, 38, Duration::from_millis(200));
        assert!(p.matches(805, 38));
    }

    #[test]
    fn pending_no_traga_otro_tamano() {
        let p = pending(805, 38, Duration::from_millis(200));
        assert!(!p.matches(1920, 1080));
    }

    #[test]
    fn pending_caduca_a_los_diez_segundos() {
        let p = pending(805, 38, Duration::from_secs(11));
        assert!(!p.is_fresh());
        assert!(!p.matches(805, 38));
    }

    fn text(content: &str, at: u64) -> ClipboardItem {
        ClipboardItem {
            id: format!("id-{content}"),
            kind: ClipboardKind::Text,
            preview: content.into(),
            text: Some(content.into()),
            image_path: None,
            created_at_ms: at,
            pinned: false,
            fingerprint: fingerprint_text(content),
            source: String::new(),
            source_app: None,
        }
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("atic-clipboard-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn lo_repetido_sube_sin_duplicarse_y_se_guarda() {
        let dir = temp_dir("repetido");
        let mut history = History::default();
        history.push_item(&dir, text("a", 1));
        history.push_item(&dir, text("b", 2));
        history.push_item(&dir, text("a", 3));
        let order: Vec<_> = history.items.iter().map(|i| i.preview.as_str()).collect();
        assert_eq!(order, ["a", "b"]);
        assert_eq!(history.items[0].created_at_ms, 3);
        assert_eq!(History::load(&dir).items.len(), 2);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn lo_borrado_no_vuelve_hasta_copiar_otra_cosa() {
        let dir = temp_dir("borrado");
        let mut history = History::default();
        history.deleted_fingerprints.insert(fingerprint_text("x"));
        history.push_item(&dir, text("x", 1));
        assert!(history.items.is_empty());
        history.push_item(&dir, text("y", 2));
        history.push_item(&dir, text("x", 3));
        assert_eq!(history.items.len(), 2);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn el_tope_respeta_los_fijados() {
        let dir = temp_dir("tope");
        let mut history = History::default();
        let mut pinned = text("fijado", 0);
        pinned.pinned = true;
        history.push_item(&dir, pinned);
        for n in 1..=MAX_ITEMS as u64 + 5 {
            history.push_item(&dir, text(&n.to_string(), n));
        }
        assert_eq!(history.items.len(), MAX_ITEMS);
        assert!(history.items.iter().any(|i| i.pinned));
        let _ = std::fs::remove_dir_all(dir);
    }
}
