//! Operaciones de archivos del panel Archivos: crear, renombrar, duplicar y
//! eliminar, desde el menú contextual o el teclado con una fila enfocada.
//!
//! La validación del nombre y el nombre de la copia son lógica pura (con
//! pruebas); lo demás es `std::fs` sin crates nuevos. En macOS se manda a la
//! papelera con el Finder (`osascript`); en el resto se elimina de verdad tras
//! la confirmación del diálogo (no hay papelera sin traer un crate nuevo).

use std::path::{Path, PathBuf};

use crate::i18n::t;

/// Un nombre no vale: por qué, en el idioma de ahora.
pub fn validate_name(name: &str) -> Result<(), String> {
    let name = name.trim();
    if name.is_empty() {
        return Err(t("pill.code.fileMenu.nameEmpty").to_string());
    }
    // El separador de los dos sistemas: una ruta con barras crearía o escaparía carpetas.
    if name.contains('/') || name.contains('\\') || name.contains('\0') {
        return Err(t("pill.code.fileMenu.nameInvalid").to_string());
    }
    if matches!(name, "." | "..") {
        return Err(t("pill.code.fileMenu.nameInvalid").to_string());
    }
    Ok(())
}

/// El destino de duplicar `path`: «nombre copia.ext», y si ya existe, «nombre
/// copia 2.ext», «nombre copia 3.ext»… `exists` mira el disco (en las pruebas,
/// una lista).
pub fn duplicate_name(path: &Path, exists: &dyn Fn(&Path) -> bool) -> PathBuf {
    let parent = path.parent();
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let (stem, ext) = match name.rfind('.').filter(|&at| at > 0) {
        Some(at) => (name[..at].to_string(), name[at..].to_string()),
        None => (name, String::new()),
    };
    let mut candidate = format!("{stem} copia{ext}");
    let mut n = 2;
    while exists(&parent.unwrap_or_else(|| Path::new("")).join(&candidate)) {
        candidate = format!("{stem} copia {n}{ext}");
        n += 1;
    }
    parent.unwrap_or_else(|| Path::new("")).join(candidate)
}

/// `path` está dentro de alguna carpeta del espacio: fuera de ellas, rechazar.
pub fn within_space(path: &Path, roots: &[PathBuf]) -> bool {
    roots.iter().any(|root| path == root || path.starts_with(root))
}

/// Crea un archivo vacío (o una carpeta) con ese nombre dentro de `dir`.
pub fn create_file(dir: &Path, name: &str) -> Result<PathBuf, String> {
    let name = name.trim();
    validate_name(name)?;
    let path = dir.join(name);
    if path.exists() {
        return Err(t("pill.code.fileMenu.nameExists").to_string());
    }
    std::fs::write(&path, "").map_err(|error| error.to_string())?;
    Ok(path)
}

/// Crea una carpeta con ese nombre dentro de `dir` (con sus padres si faltan).
pub fn create_dir(dir: &Path, name: &str) -> Result<PathBuf, String> {
    let name = name.trim();
    validate_name(name)?;
    let path = dir.join(name);
    if path.exists() {
        return Err(t("pill.code.fileMenu.nameExists").to_string());
    }
    std::fs::create_dir_all(&path).map_err(|error| error.to_string())?;
    Ok(path)
}

/// Cambia el nombre de `target` a `new_name` dentro de su misma carpeta.
pub fn rename_path(target: &Path, new_name: &str) -> Result<PathBuf, String> {
    let new_name = new_name.trim();
    validate_name(new_name)?;
    let parent = target.parent().ok_or_else(|| t("pill.code.fileMenu.outsideSpace").to_string())?;
    let dest = parent.join(new_name);
    if dest == target {
        return Ok(target.to_path_buf());
    }
    if dest.exists() {
        return Err(t("pill.code.fileMenu.nameExists").to_string());
    }
    std::fs::rename(target, &dest).map_err(|error| error.to_string())?;
    Ok(dest)
}

/// Copia un archivo (o una carpeta entera) junto al original, con el nombre de la copia.
pub fn duplicate_path(path: &Path) -> Result<PathBuf, String> {
    let dest = duplicate_name(path, &|p| p.exists());
    if path.is_dir() {
        copy_dir(path, &dest).map_err(|error| error.to_string())?;
    } else {
        std::fs::copy(path, &dest).map_err(|error| error.to_string())?;
    }
    Ok(dest)
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)?.flatten() {
        let dest = to.join(entry.file_name());
        if entry.file_type().is_ok_and(|t| t.is_dir()) {
            copy_dir(&entry.path(), &dest)?;
        } else {
            std::fs::copy(entry.path(), dest)?;
        }
    }
    Ok(())
}

/// Adónde fue a parar lo eliminado: a la papelera o fuera del todo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Removed {
    Trashed,
    Gone,
}

/// Manda `path` a la papelera (solo macOS, con el Finder) o lo elimina del
/// todo en el resto: sin crate nuevo no hay papelera segura ahí.
pub fn remove_path(path: &Path) -> Result<Removed, String> {
    #[cfg(target_os = "macos")]
    {
        if trash_with_finder(path) {
            return Ok(Removed::Trashed);
        }
        // Si el Finder no pudo, no se borra nada: se avisa.
        return Err(t("pill.code.fileMenu.trashFailed").to_string());
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = path;
        remove_permanent(path).map(|_| Removed::Gone)
    }
}

#[cfg(not(target_os = "macos"))]
fn remove_permanent(path: &Path) -> Result<(), String> {
    let result = if path.is_dir() && !path.is_symlink() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    };
    result.map_err(|error| error.to_string())
}

/// `tell application "Finder" to delete POSIX file "…"`: la papelera de macOS.
#[cfg(target_os = "macos")]
fn trash_with_finder(path: &Path) -> bool {
    let quoted = path.display().to_string().replace('\\', "\\\\").replace('"', "\\\"");
    let script = format!("tell application \"Finder\" to delete POSIX file \"{quoted}\"");
    let mut command = std::process::Command::new("osascript");
    command.args(["-e", &script]);
    command.output().is_ok_and(|out| out.status.success())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_nombre_vacio_o_con_barras_no_vale() {
        assert!(validate_name("").is_err());
        assert!(validate_name("   ").is_err());
        assert!(validate_name(".").is_err());
        assert!(validate_name("..").is_err());
        assert!(validate_name("a/b").is_err());
        assert!(validate_name("a\\b").is_err());
        assert!(validate_name("hola.rs").is_ok());
        // Los espacios de los bordes se perdonan.
        assert!(validate_name("  hola.rs  ").is_ok());
    }

    #[test]
    fn la_copia_lleva_copia_y_numero_si_ya_existe() {
        let dir = Path::new("/tmp/espacio");
        let taken = |names: &[&str]| {
            let owned: Vec<PathBuf> = names.iter().map(|n| dir.join(n)).collect();
            move |p: &Path| owned.iter().any(|o| o == p)
        };
        assert_eq!(duplicate_name(&dir.join("hola.rs"), &taken(&[])), dir.join("hola copia.rs"));
        assert_eq!(duplicate_name(&dir.join("hola.rs"), &taken(&["hola copia.rs"])), dir.join("hola copia 2.rs"));
        assert_eq!(duplicate_name(&dir.join("notas"), &taken(&["notas copia"])), dir.join("notas copia 2"));
        // Sin extensión no se parte por el punto inicial (`.env` no es «extensión vacía»).
        assert_eq!(duplicate_name(&dir.join(".env"), &taken(&[])), dir.join(".env copia"));
    }

    #[test]
    fn fuera_del_espacio_se_rechaza() {
        let roots = vec![PathBuf::from("/tmp/espacio")];
        assert!(within_space(Path::new("/tmp/espacio/src/a.rs"), &roots));
        assert!(within_space(Path::new("/tmp/espacio"), &roots));
        assert!(!within_space(Path::new("/tmp/otro/a.rs"), &roots));
        assert!(!within_space(Path::new("/tmp/espacio-falso/a.rs"), &roots));
    }

    #[test]
    fn crear_renombrar_y_duplicar_en_disco() {
        let dir = std::env::temp_dir().join(format!("atic-file-ops-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = create_file(&dir, "hola.rs").unwrap();
        assert!(file.is_file());
        assert!(create_file(&dir, "hola.rs").is_err());
        let sub = create_dir(&dir, "src").unwrap();
        assert!(sub.is_dir());
        std::fs::write(&file, "hola").unwrap();
        let dup = duplicate_path(&file).unwrap();
        assert_eq!(dup.file_name().unwrap(), "hola copia.rs");
        assert_eq!(std::fs::read_to_string(&dup).unwrap(), "hola");
        let renamed = rename_path(&file, "adios.rs").unwrap();
        assert!(!file.exists() && renamed.is_file());
        // Renombrar al mismo nombre no falla ni hace nada.
        assert_eq!(rename_path(&renamed, "adios.rs").unwrap(), renamed);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn duplicar_una_carpeta_copia_su_contenido() {
        let dir = std::env::temp_dir().join(format!("atic-file-dup-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("web/css")).unwrap();
        std::fs::write(dir.join("web/css/a.css"), "x").unwrap();
        let dup = duplicate_path(&dir.join("web")).unwrap();
        assert_eq!(std::fs::read_to_string(dup.join("css/a.css")).unwrap(), "x");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn eliminar_fuera_de_macos_borra_del_todo() {
        let dir = std::env::temp_dir().join(format!("atic-file-del-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("sub/a.txt"), "x").unwrap();
        #[cfg(not(target_os = "macos"))]
        {
            assert_eq!(remove_path(&dir.join("sub/a.txt")).unwrap(), Removed::Gone);
            assert!(!dir.join("sub/a.txt").exists());
            assert_eq!(remove_path(&dir.join("sub")).unwrap(), Removed::Gone);
            assert!(!dir.join("sub").exists());
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
