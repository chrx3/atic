//! Dónde guarda la pill lo suyo: `<datos de Atic>\pill` (`AppDirs`), junto a
//! la base, la configuración y las capturas de Atic.
//!
//! Antes vivía en `%LOCALAPPDATA%\atic-gpui`, que además era la carpeta de
//! compilación. [`migrate`] copia de ahí lo que falte, una sola vez y sin
//! borrar nada: el original queda como respaldo.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Los archivos propios de la pill que se copian desde la carpeta vieja.
const LEGACY_FILES: &[&str] = &[
    "local.json",
    "snippets-local.json",
    "launcher.json",
    "colors.json",
    "tools.json",
    "usage.txt",
    "media.txt",
    "home.txt",
    "agents.json",
    "appearance.json",
    "capture-mode.txt",
];

/// Marca de que la migración ya corrió: si el usuario borra un archivo nuevo,
/// no vuelve a aparecer el viejo.
const MIGRATED_MARK: &str = ".migrated-from-atic-gpui";

fn dirs() -> Option<&'static atic_core::AppDirs> {
    static DIRS: OnceLock<Option<atic_core::AppDirs>> = OnceLock::new();
    DIRS.get_or_init(|| atic_core::AppDirs::new().ok()).as_ref()
}

/// `<datos de Atic>\pill`.
pub fn data_dir() -> Option<PathBuf> {
    Some(dirs()?.data_dir().join("pill"))
}

/// Un archivo de la pill.
pub fn file(name: &str) -> Option<PathBuf> {
    Some(data_dir()?.join(name))
}

/// Las capturas: las mismas de Atic.
pub fn captures_dir() -> Option<PathBuf> {
    Some(dirs()?.captures_dir())
}

/// La carpeta de logs de Atic.
pub fn logs_dir() -> Option<PathBuf> {
    Some(dirs()?.logs_dir())
}

fn legacy_dir() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join("atic-gpui"))
}

/// Copia los datos de `%LOCALAPPDATA%\atic-gpui` que todavía no estén en la
/// carpeta nueva. Devuelve cuántos archivos copió.
pub fn migrate() -> usize {
    let (Some(legacy), Some(data), Some(captures)) = (legacy_dir(), data_dir(), captures_dir())
    else {
        return 0;
    };
    migrate_from(&legacy, &data, &captures)
}

fn migrate_from(legacy: &Path, data: &Path, captures: &Path) -> usize {
    if !legacy.is_dir() || data.join(MIGRATED_MARK).exists() {
        return 0;
    }
    if std::fs::create_dir_all(data).is_err() {
        return 0;
    }
    let mut copied = 0;
    for name in LEGACY_FILES {
        copied += usize::from(copy_missing(&legacy.join(name), &data.join(name)));
    }
    if let Ok(entries) = std::fs::read_dir(legacy.join("captures")) {
        let _ = std::fs::create_dir_all(captures);
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                copied += usize::from(copy_missing(&path, &captures.join(entry.file_name())));
            }
        }
    }
    let _ = std::fs::write(data.join(MIGRATED_MARK), legacy.display().to_string());
    copied
}

fn copy_missing(from: &Path, to: &Path) -> bool {
    from.is_file() && !to.exists() && std::fs::copy(from, to).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("pill-paths-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn copia_lo_que_falta_sin_pisar_ni_borrar() {
        let root = temp("copia");
        let (legacy, data, captures) = (root.join("old"), root.join("pill"), root.join("caps"));
        std::fs::create_dir_all(legacy.join("captures")).unwrap();
        std::fs::create_dir_all(&data).unwrap();
        std::fs::write(legacy.join("colors.json"), "viejo").unwrap();
        std::fs::write(legacy.join("home.txt"), "viejo").unwrap();
        std::fs::write(data.join("home.txt"), "nuevo").unwrap();
        std::fs::write(legacy.join("captures").join("a.png"), "png").unwrap();

        assert_eq!(migrate_from(&legacy, &data, &captures), 2);
        assert_eq!(std::fs::read_to_string(data.join("colors.json")).unwrap(), "viejo");
        assert_eq!(std::fs::read_to_string(data.join("home.txt")).unwrap(), "nuevo");
        assert!(captures.join("a.png").exists());
        assert!(legacy.join("colors.json").exists(), "el original queda");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn corre_una_sola_vez() {
        let root = temp("una-vez");
        let (legacy, data, captures) = (root.join("old"), root.join("pill"), root.join("caps"));
        std::fs::create_dir_all(&legacy).unwrap();
        std::fs::write(legacy.join("tools.json"), "{}").unwrap();
        assert_eq!(migrate_from(&legacy, &data, &captures), 1);
        std::fs::remove_file(data.join("tools.json")).unwrap();
        assert_eq!(migrate_from(&legacy, &data, &captures), 0);
        assert!(!data.join("tools.json").exists());
        let _ = std::fs::remove_dir_all(root);
    }
}
