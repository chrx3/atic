//! Los archivos que cambiaron en las carpetas de un agente desde que se abrió,
//! y cómo abrirlos.
//!
//! Sale de `git status`: al empezar se anota lo que ya estaba modificado (con
//! su fecha) y después cuenta lo nuevo o lo que se volvió a tocar. No sabe
//! quién escribió: dos agentes en el mismo repositorio ven los cambios del
//! otro. Una carpeta que no es un repositorio no muestra nada.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

/// Lo que está modificado en un momento: ruta absoluta → su fecha (ninguna si
/// se borró).
pub type Snapshot = HashMap<PathBuf, Option<SystemTime>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Modified,
    Added,
    Deleted,
}

#[derive(Clone, Debug)]
pub struct Change {
    pub path: PathBuf,
    /// La ruta corta para mostrar, desde la carpeta donde trabaja el agente.
    pub shown: String,
    pub kind: Kind,
    when: Option<SystemTime>,
}

/// Lo que se sabe de los cambios de una consola.
#[derive(Default)]
pub struct Tracked {
    /// Lo que ya estaba modificado al abrirla. `None` hasta la primera mirada.
    baseline: Option<Snapshot>,
    kinds: HashMap<PathBuf, Kind>,
    pub list: Vec<Change>,
}

impl Tracked {
    /// Pone al día la lista con una mirada nueva. La primera es la base.
    pub fn update(&mut self, scan: Scan, cwd: &Path) {
        let Some(baseline) = &self.baseline else {
            self.baseline = Some(scan.snapshot);
            return;
        };
        self.kinds = scan.kinds;
        self.list = diff(baseline, &scan.snapshot, &self.kinds, cwd);
    }
}

fn hidden(program: &str) -> Command {
    let mut command = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}

pub struct Scan {
    snapshot: Snapshot,
    kinds: HashMap<PathBuf, Kind>,
}

/// Mira qué está modificado en estas carpetas. Corre `git`: va en segundo plano.
pub fn scan(dirs: &[PathBuf]) -> Scan {
    let mut snapshot = Snapshot::new();
    let mut kinds = HashMap::new();
    for dir in dirs {
        let Some(root) = repo_root(dir) else {
            continue;
        };
        let Ok(output) = hidden("git")
            .arg("-C")
            .arg(dir)
            .args(["status", "--porcelain=v1", "-z", "--untracked-files=all", "--", "."])
            .output()
        else {
            continue;
        };
        if !output.status.success() {
            continue;
        }
        for (path, kind) in parse_status(&output.stdout) {
            let full = root.join(path);
            let when = std::fs::metadata(&full).and_then(|m| m.modified()).ok();
            snapshot.insert(full.clone(), when);
            kinds.insert(full, kind);
        }
    }
    Scan { snapshot, kinds }
}

fn repo_root(dir: &Path) -> Option<PathBuf> {
    let output = hidden("git").arg("-C").arg(dir).args(["rev-parse", "--show-toplevel"]).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let root = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!root.is_empty()).then(|| PathBuf::from(root))
}

/// `git status --porcelain=v1 -z`: `XY ruta\0`, y los renombres traen además
/// la ruta vieja en la entrada siguiente.
fn parse_status(raw: &[u8]) -> Vec<(String, Kind)> {
    let mut out = Vec::new();
    let mut entries = raw.split(|b| *b == 0).filter(|e| e.len() > 3);
    while let Some(entry) = entries.next() {
        let (x, y) = (entry[0], entry[1]);
        let path = String::from_utf8_lossy(&entry[3..]).into_owned();
        let kind = if x == b'?' || x == b'A' {
            Kind::Added
        } else if x == b'D' || y == b'D' {
            Kind::Deleted
        } else {
            Kind::Modified
        };
        if x == b'R' || x == b'C' {
            entries.next();
        }
        out.push((path, kind));
    }
    out
}

/// Lo que cambió respecto de la base: lo que no estaba o se volvió a tocar.
/// Lo más reciente primero.
fn diff(baseline: &Snapshot, now: &Snapshot, kinds: &HashMap<PathBuf, Kind>, cwd: &Path) -> Vec<Change> {
    let mut list: Vec<Change> = now
        .iter()
        .filter(|(path, when)| baseline.get(*path) != Some(when))
        .map(|(path, when)| Change {
            shown: shown(path, cwd),
            path: path.clone(),
            kind: kinds.get(path).copied().unwrap_or(Kind::Modified),
            when: *when,
        })
        .collect();
    list.sort_by(|a, b| b.when.cmp(&a.when).then_with(|| a.shown.cmp(&b.shown)));
    list
}

fn shown(path: &Path, cwd: &Path) -> String {
    let rel = path.strip_prefix(cwd).ok().map(Path::to_path_buf).or_else(|| {
        // De otra carpeta del espacio: con el nombre de esa carpeta delante.
        let parent = cwd.parent()?;
        path.strip_prefix(parent).ok().map(Path::to_path_buf)
    });
    rel.unwrap_or_else(|| path.to_path_buf()).to_string_lossy().replace('\\', "/")
}

/// Abre en VS Code: las carpetas como un espacio de trabajo o un archivo.
pub fn open_in_code(paths: &[PathBuf], goto: bool) {
    let mut command = hidden("cmd");
    command.args(["/C", "code"]);
    if goto {
        command.arg("-g");
    }
    command.args(paths);
    if let Err(error) = command.spawn() {
        tracing::warn!(%error, "espacio: no se pudo abrir VS Code");
    }
}

pub fn open_folder(path: &Path) {
    if let Err(error) = Command::new("explorer.exe").arg(path).spawn() {
        tracing::warn!(%error, "espacio: no se pudo abrir la carpeta");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn lee_el_estado_de_git_con_renombres() {
        let raw = b" M src/a.rs\0?? nuevo.txt\0R  b.rs\0viejo.rs\0 D borrado.rs\0";
        let parsed = parse_status(raw);
        assert_eq!(
            parsed,
            vec![
                ("src/a.rs".to_string(), Kind::Modified),
                ("nuevo.txt".to_string(), Kind::Added),
                ("b.rs".to_string(), Kind::Modified),
                ("borrado.rs".to_string(), Kind::Deleted),
            ]
        );
    }

    #[test]
    fn solo_cuenta_lo_nuevo_o_lo_que_se_volvio_a_tocar() {
        let t0 = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let t1 = t0 + Duration::from_secs(5);
        let cwd = Path::new(r"C:\repo");
        let base: Snapshot = [(cwd.join("viejo.rs"), Some(t0)), (cwd.join("tocado.rs"), Some(t0))].into();
        let now: Snapshot = [
            (cwd.join("viejo.rs"), Some(t0)),
            (cwd.join("tocado.rs"), Some(t1)),
            (cwd.join("src").join("nuevo.rs"), Some(t1)),
        ]
        .into();
        let mut shown: Vec<String> = diff(&base, &now, &HashMap::new(), cwd).into_iter().map(|c| c.shown).collect();
        shown.sort();
        assert_eq!(shown, vec!["src/nuevo.rs", "tocado.rs"]);
    }

    #[test]
    fn un_archivo_de_otra_carpeta_lleva_su_nombre() {
        let cwd = Path::new(r"C:\code\web");
        assert_eq!(shown(Path::new(r"C:\code\api\src\main.rs"), cwd), "api/src/main.rs");
        assert_eq!(shown(Path::new(r"C:\code\web\index.ts"), cwd), "index.ts");
    }
}
