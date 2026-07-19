//! Limpieza de capturas por antigüedad.
//!
//! Sigue el mismo resguardo que la retención de grabaciones
//! (`apps/desktop/src-tauri/src/retention.rs`): antes de borrar, canonicaliza
//! la ruta y verifica que su directorio padre es la carpeta de capturas, para
//! evitar escapes de directorio.

use std::path::Path;
use std::time::{Duration, SystemTime};

#[derive(Debug, Default)]
pub struct RetentionResult {
    pub deleted: usize,
    pub bytes_freed: u64,
    pub errors: Vec<String>,
}

/// Elimina los PNG de `dir` con más de `max_age_hours` horas de antigüedad.
///
/// `max_age_hours == 0` desactiva la limpieza (conservar siempre). `now` se
/// recibe como parámetro para poder probar el umbral sin manipular mtimes.
pub fn cleanup_captures(dir: &Path, max_age_hours: u32, now: SystemTime) -> RetentionResult {
    let mut result = RetentionResult::default();
    if max_age_hours == 0 {
        return result;
    }
    let max_age = Duration::from_secs(u64::from(max_age_hours) * 3600);

    let canonical_dir = match std::fs::canonicalize(dir) {
        Ok(dir) => dir,
        // Si el directorio aún no existe, no hay nada que limpiar.
        Err(_) => return result,
    };

    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) => {
            result.errors.push(error.to_string());
            return result;
        }
    };

    for entry in entries.filter_map(std::result::Result::ok) {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("png") {
            continue;
        }
        let metadata = match entry.metadata() {
            Ok(metadata) if metadata.is_file() => metadata,
            _ => continue,
        };
        let modified = metadata.modified().unwrap_or(now);
        let age = now.duration_since(modified).unwrap_or(Duration::ZERO);
        if age <= max_age {
            continue;
        }

        match std::fs::canonicalize(&path) {
            Ok(resolved) if resolved.parent() == Some(canonical_dir.as_path()) => {
                let bytes = metadata.len();
                match std::fs::remove_file(&resolved) {
                    Ok(()) => {
                        result.deleted += 1;
                        result.bytes_freed = result.bytes_freed.saturating_add(bytes);
                    }
                    Err(error) => result
                        .errors
                        .push(format!("{}: {error}", path.display())),
                }
            }
            Ok(_) => result.errors.push(format!(
                "{}: ruta fuera del directorio permitido",
                path.display()
            )),
            Err(error) => result.errors.push(format!("{}: {error}", path.display())),
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::UNIX_EPOCH;

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("atic-capture-{tag}-{nonce}"));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn zero_hours_disables_cleanup() {
        let dir = temp_dir("noop");
        std::fs::write(dir.join("capture_x.png"), [0u8; 4]).unwrap();
        let result = cleanup_captures(&dir, 0, SystemTime::now());
        assert_eq!(result.deleted, 0);
        assert!(dir.join("capture_x.png").exists());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn deletes_only_old_png_files() {
        let dir = temp_dir("age");
        std::fs::write(dir.join("old.png"), [0u8; 10]).unwrap();
        std::fs::write(dir.join("keep.txt"), [0u8; 10]).unwrap();

        // `now` en el futuro → los archivos cuentan como antiguos.
        let future = SystemTime::now() + Duration::from_secs(48 * 3600);
        let result = cleanup_captures(&dir, 24, future);

        assert_eq!(result.deleted, 1);
        assert_eq!(result.bytes_freed, 10);
        assert!(result.errors.is_empty());
        assert!(!dir.join("old.png").exists());
        // El .txt no se toca aunque sea "antiguo".
        assert!(dir.join("keep.txt").exists());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn keeps_recent_files() {
        let dir = temp_dir("recent");
        std::fs::write(dir.join("fresh.png"), [0u8; 10]).unwrap();
        // Umbral enorme → nada vence.
        let result = cleanup_captures(&dir, 100_000, SystemTime::now());
        assert_eq!(result.deleted, 0);
        assert!(dir.join("fresh.png").exists());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn missing_directory_is_noop() {
        let dir = std::env::temp_dir().join("atic-capture-does-not-exist-zzz");
        let result = cleanup_captures(&dir, 24, SystemTime::now());
        assert_eq!(result.deleted, 0);
        assert!(result.errors.is_empty());
    }
}
