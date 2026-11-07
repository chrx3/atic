//! Los archivos del proyecto de un agente que el celular puede pedir
//! ([`crate::proto::ToDesktop::FetchFile`]) y los artefactos que deja un turno
//! ([`crate::proto::ToPhone::ArtifactReady`]).
//!
//! El celular manda solo una ruta relativa. Acá se resuelve contra la raíz del
//! proyecto y se rechaza todo lo que pueda salir de ella o tocar algo sensible:
//! `..`, rutas absolutas, unidades y UNC, symlinks que apuntan afuera, lo que no
//! es un archivo regular, la lista de bloqueo (`.env`, llaves, `.git/`…) y lo que
//! pasa el tope de tamaño.

use std::path::{Component, Path, PathBuf};
use std::time::SystemTime;

use crate::proto::MAX_FILE_BYTES;

/// Por qué no se entrega un archivo. El texto va tal cual al celular.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// La ruta no es relativa o trae `..`, una unidad, UNC o caracteres raros.
    BadPath,
    /// Está en la lista de bloqueo.
    Blocked,
    /// No existe, o no es un archivo regular.
    NotFound,
    /// Resuelta (por un symlink) queda fuera de la raíz del proyecto.
    Outside,
    TooBig,
    /// No está en la última lista que se le mostró al celular.
    NotOffered,
}

impl Refusal {
    pub fn reason(&self) -> &'static str {
        match self {
            Self::BadPath => "La ruta no es válida.",
            Self::Blocked => "Ese archivo no se puede bajar desde el celular.",
            Self::NotFound => "El archivo ya no está.",
            Self::Outside => "El archivo está fuera del proyecto.",
            Self::TooBig => "El archivo pasa el tope de 2 GB.",
            Self::NotOffered => "Vuelve a cargar la lista de cambios.",
        }
    }
}

/// Un archivo ya revisado, listo para mandar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    pub path: PathBuf,
    pub size: u64,
}

/// Las partes de una ruta relativa que manda el celular, o `None` si no es una
/// ruta relativa limpia: sin `..`, sin raíz, sin unidad (`C:`), sin UNC, sin `:`
/// (también los flujos alternos de NTFS) ni caracteres de control.
pub fn clean_parts(rel: &str) -> Option<Vec<&str>> {
    if rel.is_empty() || rel.len() > 4096 || rel.starts_with(['/', '\\']) {
        return None;
    }
    if rel.chars().any(|c| c.is_control() || c == ':') {
        return None;
    }
    let parts: Vec<&str> = rel.split(['/', '\\']).filter(|p| !p.is_empty() && *p != ".").collect();
    if parts.is_empty() || parts.iter().any(|p| *p == ".." || p.trim_end_matches([' ', '.']).is_empty()) {
        return None;
    }
    Some(parts)
}

/// Lo que nunca se entrega, aunque git no lo ignore: `.git/` entero, `.env*`,
/// llaves y certificados, `credentials*`, `secrets.*`.
pub fn is_blocked(rel: &str) -> bool {
    let parts: Vec<String> = rel.split(['/', '\\']).filter(|p| !p.is_empty()).map(str::to_ascii_lowercase).collect();
    if parts.iter().any(|p| p == ".git") {
        return true;
    }
    let Some(name) = parts.last() else {
        return true;
    };
    name.starts_with(".env")
        || name.starts_with("id_rsa")
        || name.starts_with("id_ed25519")
        || name.starts_with("id_ecdsa")
        || name.starts_with("credentials")
        || name.starts_with("secrets.")
        || [".pem", ".key", ".p12", ".pfx", ".keystore", ".jks"].iter().any(|ext| name.ends_with(ext))
        || name == ".npmrc"
        || name == ".netrc"
}

/// Extensiones que se reconocen como resultado de un trabajo (se avisan al
/// terminar un turno y se ofrecen aunque git las ignore).
const ARTIFACT_EXTS: [&str; 18] =
    ["apk", "aab", "ipa", "mp4", "mov", "webm", "mkv", "gif", "png", "jpg", "jpeg", "webp", "zip", "pdf", "dmg", "msi", "mp3", "wav"];

/// Si `rel` parece un artefacto: por su extensión o por estar en `build/outputs/`.
pub fn is_artifact(rel: &str) -> bool {
    let lower = rel.replace('\\', "/").to_ascii_lowercase();
    if lower.starts_with("build/outputs/") || lower.contains("/build/outputs/") {
        return true;
    }
    lower.rsplit_once('.').is_some_and(|(_, ext)| ARTIFACT_EXTS.contains(&ext))
}

/// El tipo de un archivo por su extensión.
pub fn mime_for(name: &str) -> &'static str {
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    match ext.as_str() {
        "apk" => "application/vnd.android.package-archive",
        "aab" => "application/x-authorware-bin",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "pdf" => "application/pdf",
        "zip" => "application/zip",
        "mp4" => "video/mp4",
        "mov" => "video/quicktime",
        "webm" => "video/webm",
        "mkv" => "video/x-matroska",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "json" => "application/json",
        "html" | "htm" => "text/html",
        "md" | "txt" | "rs" | "kt" | "kts" | "java" | "ts" | "tsx" | "js" | "jsx" | "mjs" | "py" | "toml" | "yaml" | "yml" | "xml" | "css"
        | "gradle" | "sh" | "ps1" | "c" | "h" | "cpp" | "go" | "swift" | "lock" => "text/plain",
        _ => "application/octet-stream",
    }
}

/// Resuelve `rel` dentro de `root` con todas las reglas. `root` es la carpeta del
/// proyecto del agente, nunca algo que mande el celular.
pub fn resolve(root: &Path, rel: &str) -> Result<Resolved, Refusal> {
    let parts = clean_parts(rel).ok_or(Refusal::BadPath)?;
    if is_blocked(rel) {
        return Err(Refusal::Blocked);
    }
    let root = std::fs::canonicalize(root).map_err(|_| Refusal::NotFound)?;
    let mut full = root.clone();
    for part in &parts {
        full.push(part);
    }
    // Después de sumar, cada parte tiene que seguir siendo un nombre (ningún prefijo
    // ni raíz se coló por una rareza del sistema).
    if full.strip_prefix(&root).map_or(true, |rest| rest.components().any(|c| !matches!(c, Component::Normal(_)))) {
        return Err(Refusal::BadPath);
    }
    let real = std::fs::canonicalize(&full).map_err(|_| Refusal::NotFound)?;
    // Un symlink (o una unión de Windows) que lleva fuera del proyecto no pasa.
    let inside = real.strip_prefix(&root).map_err(|_| Refusal::Outside)?;
    // Y uno que lleva a algo bloqueado dentro del proyecto, tampoco.
    if is_blocked(&inside.to_string_lossy()) {
        return Err(Refusal::Blocked);
    }
    let meta = std::fs::metadata(&real).map_err(|_| Refusal::NotFound)?;
    if !meta.is_file() {
        return Err(Refusal::NotFound);
    }
    if meta.len() > MAX_FILE_BYTES {
        return Err(Refusal::TooBig);
    }
    Ok(Resolved { path: real, size: meta.len() })
}

/// Carpetas que no se recorren buscando artefactos: enormes y sin nada que bajar.
const SKIP_DIRS: [&str; 9] = [".git", "node_modules", ".gradle", ".idea", ".venv", "venv", "__pycache__", "target", ".next"];
/// Profundidad y entradas como mucho al buscar: un proyecto gigante no traba nada.
const MAX_DEPTH: usize = 12;
const MAX_VISITED: usize = 40_000;

/// Un artefacto que dejó un turno.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Artifact {
    /// Relativa a la raíz, con `/`.
    pub rel_path: String,
    pub size: u64,
    pub modified: SystemTime,
}

/// Los artefactos de `root` modificados desde `since` (aunque git los ignore),
/// lo más nuevo primero y hasta `limit`. No sigue symlinks.
pub fn find_artifacts(root: &Path, since: SystemTime, limit: usize) -> Vec<Artifact> {
    let mut found = Vec::new();
    let mut stack = vec![(root.to_path_buf(), 0usize)];
    let mut visited = 0usize;
    while let Some((dir, depth)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            visited += 1;
            if visited > MAX_VISITED {
                break;
            }
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
            if kind.is_dir() {
                if depth < MAX_DEPTH && !SKIP_DIRS.contains(&name.as_str()) {
                    stack.push((path, depth + 1));
                }
                continue;
            }
            if !kind.is_file() {
                continue;
            }
            let Ok(rel) = path.strip_prefix(root) else {
                continue;
            };
            let rel = rel.to_string_lossy().replace('\\', "/");
            if !is_artifact(&rel) || is_blocked(&rel) {
                continue;
            }
            let Ok(meta) = entry.metadata() else {
                continue;
            };
            let modified = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
            if modified >= since && meta.len() > 0 && meta.len() <= MAX_FILE_BYTES {
                found.push(Artifact { rel_path: rel, size: meta.len(), modified });
            }
        }
        if visited > MAX_VISITED {
            break;
        }
    }
    found.sort_by(|a, b| b.modified.cmp(&a.modified));
    found.truncate(limit);
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solo_rutas_relativas_limpias() {
        assert_eq!(clean_parts("src/main.rs"), Some(vec!["src", "main.rs"]));
        assert_eq!(clean_parts("./app\\build/x.apk"), Some(vec!["app", "build", "x.apk"]));
        for bad in ["", "/etc/passwd", "\\\\server\\share\\x", "C:\\Windows\\win.ini", "c:x", "../x", "a/../../x", "a/..", "a/ .. /b", "a\u{0}b", "a/file.txt:secreto", "...", "a/. ."] {
            assert_eq!(clean_parts(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn la_lista_de_bloqueo() {
        for blocked in [".env", ".env.local", "config/.ENV.production", "keys/server.pem", "a.key", "id_rsa", "id_rsa.pub", "cert.p12", "release.keystore", "upload.jks", "credentials.json", "secrets.toml", ".git/config", "sub/.git/HEAD"] {
            assert!(is_blocked(blocked), "{blocked}");
        }
        for ok in ["src/env.rs", "environment.md", "app/build/outputs/apk/app-release.apk", "keyboard.rs", "docs/secrets-guide.md"] {
            assert!(!is_blocked(ok), "{ok}");
        }
    }

    #[test]
    fn reconoce_los_artefactos() {
        assert!(is_artifact("app/build/outputs/apk/debug/app-debug.apk"));
        assert!(is_artifact("build/outputs/mapping/release/mapping.txt"));
        assert!(is_artifact("demo.MP4"));
        assert!(is_artifact("docs/informe.pdf"));
        assert!(!is_artifact("src/main.rs"));
        assert!(!is_artifact("outputs/build.rs"));
        assert_eq!(mime_for("app-release.apk"), "application/vnd.android.package-archive");
    }
}
