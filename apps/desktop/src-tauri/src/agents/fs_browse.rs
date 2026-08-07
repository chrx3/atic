//! Explorador de carpetas de solo lectura (cwd de agentes).
//!
//! Lista subdirectorios sin abrir el diálogo nativo del SO — ese picker
//! pelea con always-on-top del float de agentes.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirEntry {
    pub name: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectoryListing {
    pub path: String,
    pub parent: Option<String>,
    pub entries: Vec<DirEntry>,
    /// Atajos: Inicio, Escritorio, Documentos (si existen).
    pub roots: Vec<DirEntry>,
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
}

/// Ruta legible: sin el prefijo `\\?\` que mete `canonicalize` en Windows.
fn display_path(path: &Path) -> String {
    let raw = path.to_string_lossy();
    let trimmed = raw
        .strip_prefix(r"\\?\")
        .or_else(|| raw.strip_prefix("//?/"))
        .unwrap_or(raw.as_ref());
    trimmed.to_string()
}

fn first_existing(candidates: impl IntoIterator<Item = PathBuf>) -> Option<PathBuf> {
    candidates.into_iter().find(|p| p.is_dir())
}

fn documents_dir(home: &Path) -> Option<PathBuf> {
    let mut candidates = vec![home.join("Documents"), home.join("Documentos")];
    for key in ["OneDrive", "OneDriveConsumer", "OneDriveCommercial"] {
        if let Ok(od) = std::env::var(key) {
            let base = PathBuf::from(od);
            candidates.push(base.join("Documents"));
            candidates.push(base.join("Documentos"));
        }
    }
    first_existing(candidates)
}

fn desktop_dir(home: &Path) -> Option<PathBuf> {
    let mut candidates = vec![home.join("Desktop"), home.join("Escritorio")];
    for key in ["OneDrive", "OneDriveConsumer", "OneDriveCommercial"] {
        if let Ok(od) = std::env::var(key) {
            let base = PathBuf::from(od);
            candidates.push(base.join("Desktop"));
            candidates.push(base.join("Escritorio"));
        }
    }
    first_existing(candidates)
}

fn common_roots() -> Vec<DirEntry> {
    let Some(home) = home_dir() else {
        return Vec::new();
    };
    let mut roots = Vec::new();
    roots.push(DirEntry {
        name: "Inicio".into(),
        path: display_path(&home),
    });
    if let Some(desktop) = desktop_dir(&home) {
        roots.push(DirEntry {
            name: "Escritorio".into(),
            path: display_path(&desktop),
        });
    }
    if let Some(docs) = documents_dir(&home) {
        roots.push(DirEntry {
            name: "Documentos".into(),
            path: display_path(&docs),
        });
    }
    roots
}

fn expand_input(path: Option<&str>) -> Result<PathBuf, String> {
    let raw = path.map(str::trim).filter(|s| !s.is_empty());
    let resolved = match raw {
        None | Some("~") => home_dir().ok_or_else(|| "no se pudo resolver el home".to_string())?,
        Some(p) if p.starts_with("~/") || p.starts_with("~\\") => {
            let home = home_dir().ok_or_else(|| "no se pudo resolver el home".to_string())?;
            home.join(&p[2..])
        }
        Some(p) => PathBuf::from(p),
    };

    let abs = if resolved.is_absolute() {
        resolved
    } else {
        let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
        cwd.join(resolved)
    };

    if !abs.exists() {
        return Err(format!("no existe: {}", display_path(&abs)));
    }
    if !abs.is_dir() {
        return Err(format!("no es una carpeta: {}", display_path(&abs)));
    }
    Ok(abs)
}

fn parent_of(path: &Path) -> Option<String> {
    path.parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(display_path)
}

/// Lista solo subdirectorios de `path` (vacío/`~` → home).
pub fn list_directories(path: Option<String>) -> Result<DirectoryListing, String> {
    let dir = expand_input(path.as_deref())?;
    let mut entries = Vec::new();

    let read = fs::read_dir(&dir).map_err(|e| format!("no se pudo leer {}: {e}", display_path(&dir)))?;
    for item in read.flatten() {
        let meta = match item.file_type() {
            Ok(t) => t,
            Err(_) => continue,
        };
        // Solo directorios; no seguimos symlinks a archivos.
        if !meta.is_dir() {
            continue;
        }
        let name = item.file_name().to_string_lossy().to_string();
        // Ocultos de Unix; en Windows los “dot dirs” son raros pero se filtran igual.
        if name.starts_with('.') {
            continue;
        }
        entries.push(DirEntry {
            name,
            path: display_path(&item.path()),
        });
    }

    entries.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

    Ok(DirectoryListing {
        path: display_path(&dir),
        parent: parent_of(&dir),
        entries,
        roots: common_roots(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_temp_subdir() {
        let dir = std::env::temp_dir().join(format!("atic-fs-browse-{}", std::process::id()));
        let nested = dir.join("alpha");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&nested).unwrap();
        fs::write(dir.join("file.txt"), b"x").unwrap();

        let listing = list_directories(Some(display_path(&dir))).unwrap();
        assert_eq!(listing.path, display_path(&dir));
        assert!(listing.entries.iter().any(|e| e.name == "alpha"));
        assert!(!listing.entries.iter().any(|e| e.name == "file.txt"));
        assert!(listing.parent.is_some());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_file() {
        let file = std::env::temp_dir().join(format!("atic-fs-browse-file-{}", std::process::id()));
        fs::write(&file, b"x").unwrap();
        let err = list_directories(Some(display_path(&file))).unwrap_err();
        assert!(err.contains("no es una carpeta"));
        let _ = fs::remove_file(&file);
    }
}
