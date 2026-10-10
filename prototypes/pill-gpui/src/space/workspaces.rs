//! Los espacios de trabajo de Atic Code, como los de VS Code: un nombre y
//! varias carpetas.
//!
//! Un agente se abre en un espacio: arranca en la primera carpeta y recibe las
//! demás con `--add-dir` (Claude y Codex), así lee y edita en todas. La barra
//! lateral agrupa a los agentes por espacio. Se guardan en
//! `space-workspaces.json`; la lista de carpetas de antes (`space-folders.json`)
//! pasa a ser un espacio.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
pub struct Workspace {
    pub id: u64,
    pub name: String,
    pub folders: Vec<PathBuf>,
    /// En la barra lateral se ven solo el nombre y sus agentes.
    #[serde(default)]
    pub collapsed: bool,
}

impl Workspace {
    /// Donde arranca un agente nuevo.
    pub fn main(&self) -> Option<&PathBuf> {
        self.folders.first()
    }

    /// Las otras carpetas, para un agente que arranca en `cwd`.
    pub fn extras(&self, cwd: &Path) -> Vec<PathBuf> {
        self.folders.iter().filter(|f| !same(f, cwd)).cloned().collect()
    }

    pub fn contains(&self, path: &Path) -> bool {
        self.folders.iter().any(|f| same(f, path))
    }
}

#[derive(Default, Serialize, Deserialize)]
pub struct Workspaces {
    list: Vec<Workspace>,
    active: Option<u64>,
    next: u64,
}

/// La lista de carpetas de la versión anterior.
#[derive(Deserialize)]
struct OldFolders {
    list: Vec<PathBuf>,
}

fn file(name: &str) -> Option<PathBuf> {
    crate::paths::file(name)
}

impl Workspaces {
    pub fn load() -> Self {
        let read = |name: &str| file(name).and_then(|path| std::fs::read_to_string(path).ok());
        let mut spaces: Self = match read("space-workspaces.json") {
            Some(text) => serde_json::from_str(&text).unwrap_or_default(),
            None => {
                let old: Option<OldFolders> = read("space-folders.json").and_then(|t| serde_json::from_str(&t).ok());
                let mut spaces = Self::default();
                if let Some(old) = old.filter(|old| !old.list.is_empty()) {
                    spaces.create(old.list);
                }
                spaces
            }
        };
        for space in &mut spaces.list {
            space.folders.retain(|f| f.is_dir());
        }
        if spaces.active.is_none_or(|id| spaces.get(id).is_none()) {
            spaces.active = spaces.list.first().map(|s| s.id);
        }
        spaces
    }

    fn save(&self) {
        let Some(path) = file("space-workspaces.json") else {
            return;
        };
        match serde_json::to_string_pretty(self) {
            Ok(text) => {
                if let Err(error) = std::fs::write(&path, text) {
                    tracing::warn!(%error, "espacio: no se guardaron los espacios");
                }
            }
            Err(error) => tracing::warn!(%error, "espacio: espacios sin serializar"),
        }
    }

    pub fn list(&self) -> &[Workspace] {
        &self.list
    }

    pub fn get(&self, id: u64) -> Option<&Workspace> {
        self.list.iter().find(|s| s.id == id)
    }

    fn get_mut(&mut self, id: u64) -> Option<&mut Workspace> {
        self.list.iter_mut().find(|s| s.id == id)
    }

    pub fn active_id(&self) -> Option<u64> {
        self.active
    }

    pub fn active(&self) -> Option<&Workspace> {
        self.active.and_then(|id| self.get(id))
    }

    /// El espacio de una carpeta: el activo si la tiene, si no el primero.
    pub fn find_for(&self, cwd: &Path) -> Option<u64> {
        if let Some(active) = self.active().filter(|s| s.contains(cwd)) {
            return Some(active.id);
        }
        self.list.iter().find(|s| s.contains(cwd)).map(|s| s.id)
    }

    pub fn select(&mut self, id: u64) {
        if self.get(id).is_some() && self.active != Some(id) {
            self.active = Some(id);
            self.save();
        }
    }

    pub fn create(&mut self, folders: Vec<PathBuf>) -> u64 {
        self.next = self.next.max(self.list.iter().map(|s| s.id + 1).max().unwrap_or(0));
        let id = self.next;
        self.next += 1;
        let mut unique: Vec<PathBuf> = Vec::new();
        for folder in folders {
            if !unique.iter().any(|f| same(f, &folder)) {
                unique.push(folder);
            }
        }
        self.list.push(Workspace { id, name: name_for(&unique), folders: unique, collapsed: false });
        self.active = Some(id);
        self.save();
        id
    }

    pub fn add_folders(&mut self, id: u64, folders: Vec<PathBuf>) {
        let Some(space) = self.get_mut(id) else {
            return;
        };
        let renamed = space.name == name_for(&space.folders);
        for folder in folders {
            if !space.contains(&folder) {
                space.folders.push(folder);
            }
        }
        // El nombre puesto solo sigue a las carpetas.
        if renamed {
            space.name = name_for(&space.folders);
        }
        self.save();
    }

    pub fn remove_folder(&mut self, id: u64, index: usize) {
        let Some(space) = self.get_mut(id) else {
            return;
        };
        if index < space.folders.len() {
            let renamed = space.name == name_for(&space.folders);
            space.folders.remove(index);
            if renamed {
                space.name = name_for(&space.folders);
            }
            self.save();
        }
    }

    pub fn remove(&mut self, id: u64) {
        self.list.retain(|s| s.id != id);
        if self.active == Some(id) {
            self.active = self.list.first().map(|s| s.id);
        }
        self.save();
    }

    /// Un nombre puesto a mano (vacío: vuelve al que sale de las carpetas).
    pub fn rename(&mut self, id: u64, name: &str) {
        let Some(space) = self.get_mut(id) else {
            return;
        };
        let name = name.trim();
        space.name = if name.is_empty() { name_for(&space.folders) } else { name.to_string() };
        self.save();
    }

    pub fn set_collapsed(&mut self, id: u64, collapsed: bool) {
        if let Some(space) = self.get_mut(id).filter(|s| s.collapsed != collapsed) {
            space.collapsed = collapsed;
            self.save();
        }
    }

    pub fn toggle(&mut self, id: u64) {
        if let Some(space) = self.get_mut(id) {
            space.collapsed = !space.collapsed;
            self.save();
        }
    }
}

pub fn same(a: &Path, b: &Path) -> bool {
    a.to_string_lossy()
        .trim_end_matches(['\\', '/'])
        .eq_ignore_ascii_case(b.to_string_lossy().trim_end_matches(['\\', '/']))
}

/// El nombre de una carpeta, corto para la barra lateral.
pub fn short_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// El nombre que se pone solo: lo que comparten los nombres de las carpetas
/// (`dashboard-hvar` y `dashboard-hvar-backend` → `dashboard-hvar`), o el de la
/// primera.
pub fn name_for(folders: &[PathBuf]) -> String {
    let names: Vec<String> = folders.iter().map(|f| short_name(f)).collect();
    let Some(first) = names.first() else {
        return "Espacio".into();
    };
    let mut prefix: Vec<char> = first.chars().collect();
    for name in &names[1..] {
        let common = prefix.iter().zip(name.chars()).take_while(|(a, b)| a.eq_ignore_ascii_case(b)).count();
        prefix.truncate(common);
    }
    let prefix: String = prefix.into_iter().collect();
    let prefix = prefix.trim_end_matches(['-', '_', '.', ' ']);
    if prefix.chars().count() >= 3 {
        prefix.to_string()
    } else {
        first.clone()
    }
}

/// La línea de un agente con las otras carpetas. Claude y Codex entienden
/// `--add-dir`; los demás arrancan igual que antes.
pub fn with_add_dirs(agent: &str, line: &str, dirs: &[PathBuf]) -> String {
    if dirs.is_empty() || !matches!(agent, "claude" | "codex") {
        return line.to_string();
    }
    let mut out = line.to_string();
    for dir in dirs {
        out.push_str(&format!(" --add-dir \"{}\"", dir.display()));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(list: &[&str]) -> Vec<PathBuf> {
        list.iter().map(PathBuf::from).collect()
    }

    #[test]
    fn el_nombre_es_lo_que_comparten_las_carpetas() {
        assert_eq!(name_for(&paths(&[r"C:\gh\dashboard-hvar", r"C:\gh\dashboard-hvar-backend"])), "dashboard-hvar");
        assert_eq!(name_for(&paths(&[r"C:\gh\atic", r"C:\gh\web"])), "atic");
        assert_eq!(name_for(&[]), "Espacio");
    }

    #[test]
    fn un_agente_recibe_las_otras_carpetas_del_espacio() {
        let space = Workspace { id: 0, name: "x".into(), folders: paths(&[r"C:\a", r"C:\b", r"C:\c"]), collapsed: false };
        assert_eq!(space.extras(Path::new(r"c:\B\")), paths(&[r"C:\a", r"C:\c"]));
        assert!(space.contains(Path::new(r"C:\c")));
    }

    #[test]
    fn solo_claude_y_codex_llevan_add_dir() {
        let dirs = [PathBuf::from(r"C:\code\api")];
        assert_eq!(with_add_dirs("claude", "claude", &dirs), r#"claude --add-dir "C:\code\api""#);
        assert_eq!(with_add_dirs("codex", "codex", &dirs), r#"codex --add-dir "C:\code\api""#);
        assert_eq!(with_add_dirs("opencode", "opencode", &dirs), "opencode");
        assert_eq!(with_add_dirs("claude", "claude", &[]), "claude");
    }
}
