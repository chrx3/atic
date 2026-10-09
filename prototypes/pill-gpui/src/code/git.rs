//! Lo que muestra el panel de cambios: por cada repositorio del espacio, su
//! rama y los archivos modificados con las líneas que suman y quitan
//! (`git status` y `git diff --numstat` contra el último commit). Corre `git`:
//! va en segundo plano.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Un archivo nuevo más grande que esto no se cuenta línea por línea.
const MAX_COUNT_BYTES: u64 = 1024 * 1024;

#[derive(Clone, Debug, PartialEq)]
pub struct FileChange {
    pub path: PathBuf,
    pub name: String,
    /// La carpeta dentro del repositorio («» si está en la raíz).
    pub dir: String,
    /// `A` nuevo, `M` modificado, `D` borrado, `R` renombrado.
    pub status: char,
    pub added: usize,
    pub removed: usize,
}

#[derive(Clone, Debug)]
pub struct Repo {
    pub name: String,
    pub branch: Option<String>,
    pub files: Vec<FileChange>,
}

fn git(dir: &Path) -> Command {
    let mut command = Command::new("git");
    command.arg("-C").arg(dir);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}

fn output(command: &mut Command) -> Option<Vec<u8>> {
    let output = command.output().ok()?;
    output.status.success().then_some(output.stdout)
}

fn line(bytes: Vec<u8>) -> Option<String> {
    let text = String::from_utf8_lossy(&bytes).trim().to_string();
    (!text.is_empty()).then_some(text)
}

/// El estado de los repositorios de estas carpetas (cada uno una vez).
pub fn status(folders: &[PathBuf]) -> Vec<Repo> {
    let mut roots: Vec<PathBuf> = Vec::new();
    for folder in folders {
        let Some(root) = output(git(folder).args(["rev-parse", "--show-toplevel"])).and_then(line) else {
            continue;
        };
        let root = PathBuf::from(root);
        if !roots.contains(&root) {
            roots.push(root);
        }
    }
    roots.iter().map(|root| repo(root)).collect()
}

fn repo(root: &Path) -> Repo {
    let name = root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| root.display().to_string());
    let branch = output(git(root).args(["rev-parse", "--abbrev-ref", "HEAD"])).and_then(line);
    let entries = output(git(root).args(["status", "--porcelain=v1", "-z", "--untracked-files=all"]))
        .map(|raw| parse_status(&raw))
        .unwrap_or_default();
    let counts = output(git(root).args(["diff", "--numstat", "-z", "HEAD"]))
        .map(|raw| parse_numstat(&raw))
        .unwrap_or_default();
    let files = entries
        .into_iter()
        .map(|(rel, status)| {
            let path = root.join(&rel);
            let (added, removed) = counts.get(&rel).copied().unwrap_or_else(|| {
                if status == 'A' {
                    (count_lines(&path), 0)
                } else {
                    (0, 0)
                }
            });
            let (dir, name) = match rel.rsplit_once('/') {
                Some((dir, name)) => (dir.to_string(), name.to_string()),
                None => (String::new(), rel.clone()),
            };
            FileChange { path, name, dir, status, added, removed }
        })
        .collect();
    Repo { name, branch, files }
}

/// `git status --porcelain=v1 -z`: `XY ruta\0`; los renombres traen la ruta
/// vieja en la entrada siguiente.
fn parse_status(raw: &[u8]) -> Vec<(String, char)> {
    let mut out = Vec::new();
    let mut entries = raw.split(|b| *b == 0).filter(|e| e.len() > 3);
    while let Some(entry) = entries.next() {
        let (x, y) = (entry[0], entry[1]);
        let path = String::from_utf8_lossy(&entry[3..]).into_owned();
        let status = match (x, y) {
            (b'?', _) | (b'A', _) => 'A',
            (b'D', _) | (_, b'D') => 'D',
            (b'R', _) | (b'C', _) => {
                entries.next();
                'R'
            }
            _ => 'M',
        };
        out.push((path, status));
    }
    out
}

/// `git diff --numstat -z`: `sumadas\tquitadas\truta\0`; en un renombre la
/// ruta va vacía y siguen la vieja y la nueva. Los binarios traen `-`.
fn parse_numstat(raw: &[u8]) -> HashMap<String, (usize, usize)> {
    let mut out = HashMap::new();
    let mut parts = raw.split(|b| *b == 0);
    while let Some(entry) = parts.next() {
        let text = String::from_utf8_lossy(entry);
        let mut fields = text.splitn(3, '\t');
        let (Some(added), Some(removed), Some(path)) = (fields.next(), fields.next(), fields.next()) else {
            continue;
        };
        let path = if path.is_empty() {
            parts.next();
            parts.next().map(|p| String::from_utf8_lossy(p).into_owned()).unwrap_or_default()
        } else {
            path.to_string()
        };
        out.insert(path, (added.parse().unwrap_or(0), removed.parse().unwrap_or(0)));
    }
    out
}

fn count_lines(path: &Path) -> usize {
    match std::fs::metadata(path) {
        Ok(meta) if meta.is_file() && meta.len() <= MAX_COUNT_BYTES => std::fs::read(path)
            .ok()
            .filter(|bytes| !bytes.iter().take(8192).any(|b| *b == 0))
            .map(|bytes| bytes.split(|b| *b == b'\n').filter(|l| !l.is_empty()).count())
            .unwrap_or(0),
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lee_el_estado_con_nuevos_borrados_y_renombres() {
        let raw = b" M src/a.rs\0?? nuevo.txt\0R  b.rs\0viejo.rs\0 D borrado.rs\0A  agregado.rs\0";
        assert_eq!(
            parse_status(raw),
            vec![
                ("src/a.rs".to_string(), 'M'),
                ("nuevo.txt".to_string(), 'A'),
                ("b.rs".to_string(), 'R'),
                ("borrado.rs".to_string(), 'D'),
                ("agregado.rs".to_string(), 'A'),
            ]
        );
    }

    #[test]
    fn cuenta_las_lineas_tambien_en_renombres_y_binarios() {
        let raw = b"3\t1\tsrc/a.rs\x000\t0\t\x00viejo.rs\x00nuevo.rs\x00-\t-\tlogo.png\x00";
        let counts = parse_numstat(raw);
        assert_eq!(counts.get("src/a.rs"), Some(&(3, 1)));
        assert_eq!(counts.get("nuevo.rs"), Some(&(0, 0)));
        assert_eq!(counts.get("logo.png"), Some(&(0, 0)));
        assert_eq!(counts.len(), 3);
    }
}
