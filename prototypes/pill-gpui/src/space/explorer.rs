//! El árbol de archivos de un espacio, en el panel de la derecha del Mando.
//!
//! Las carpetas se leen al abrirlas y se vuelven a leer si lo que se tiene
//! guardado es de hace más de unos segundos: los agentes crean y borran
//! archivos todo el tiempo. Las carpetas raíz parten abiertas; `.git` no se
//! muestra. El de Atic Code (`Explorer::for_code`) tampoco muestra lo que la referencia
//! oculta: `node_modules`, `target`, `dist` y `.DS_Store`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Lo leído de una carpeta vale por este tiempo.
const FRESH_FOR: Duration = Duration::from_secs(5);
/// Con `node_modules` abierta, el árbol no se dibuja entero.
pub const MAX_ROWS: usize = 1500;

#[derive(Clone)]
struct Item {
    path: PathBuf,
    name: String,
    dir: bool,
}

pub struct Row {
    pub depth: usize,
    pub path: PathBuf,
    pub name: String,
    pub dir: bool,
    pub open: bool,
}

/// Lo que el árbol de Atic Code oculta además de `.git` (los mismos nombres de la referencia).
const BUILD_DIRS: [&str; 4] = ["node_modules", "target", "dist", ".DS_Store"];

/// Si el árbol no muestra una entrada con este nombre.
fn is_hidden(name: &str, hide_build: bool) -> bool {
    name == ".git" || (hide_build && BUILD_DIRS.contains(&name))
}

#[derive(Default)]
pub struct Explorer {
    /// Oculta también `node_modules`, `target` y `dist` (el árbol de Atic Code).
    hide_build: bool,
    /// Lo que se abrió o cerró a mano. Lo demás: abiertas las raíces.
    expanded: HashMap<PathBuf, bool>,
    cache: HashMap<PathBuf, (Instant, Vec<Item>)>,
}

fn read(dir: &Path, hide_build: bool) -> Vec<Item> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut items: Vec<Item> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            (!is_hidden(&name, hide_build)).then(|| Item { dir: entry.file_type().is_ok_and(|t| t.is_dir()), path: entry.path(), name })
        })
        .collect();
    items.sort_by(|a, b| b.dir.cmp(&a.dir).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    items
}

impl Explorer {
    /// El árbol de Atic Code: sin `.git`, `node_modules`, `target` ni `dist`.
    pub fn for_code() -> Self {
        Self { hide_build: true, ..Self::default() }
    }

    fn is_open(&self, path: &Path, root: bool) -> bool {
        self.expanded.get(path).copied().unwrap_or(root)
    }

    pub fn toggle(&mut self, path: &Path, root: bool) {
        let open = self.is_open(path, root);
        self.expanded.insert(path.to_path_buf(), !open);
    }

    pub fn refresh(&mut self) {
        self.cache.clear();
    }

    fn children(&mut self, dir: &Path, now: Instant) -> Vec<Item> {
        match self.cache.get(dir) {
            Some((at, items)) if now.duration_since(*at) < FRESH_FOR => items.clone(),
            _ => {
                let items = read(dir, self.hide_build);
                self.cache.insert(dir.to_path_buf(), (now, items.clone()));
                items
            }
        }
    }

    /// Las filas a la vista, en orden, con su sangría.
    pub fn rows(&mut self, roots: &[PathBuf]) -> Vec<Row> {
        let now = Instant::now();
        let mut out = Vec::new();
        for root in roots {
            let name = root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| root.display().to_string());
            self.walk(root, name, true, 0, now, &mut out);
        }
        out
    }

    fn walk(&mut self, path: &Path, name: String, dir: bool, depth: usize, now: Instant, out: &mut Vec<Row>) {
        if out.len() >= MAX_ROWS {
            return;
        }
        let open = dir && self.is_open(path, depth == 0);
        out.push(Row { depth, path: path.to_path_buf(), name, dir, open });
        if open {
            for item in self.children(path, now) {
                self.walk(&item.path, item.name, item.dir, depth + 1, now, out);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_arbol_abre_las_raices_y_lo_que_se_pide() {
        let root = std::env::temp_dir().join(format!("atic-explorer-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::write(root.join("b.txt"), "").unwrap();
        std::fs::write(root.join("src").join("main.rs"), "").unwrap();

        let mut explorer = Explorer::default();
        let names = |rows: Vec<Row>| rows.into_iter().map(|r| format!("{}{}", "  ".repeat(r.depth), r.name)).collect::<Vec<_>>();
        let top = root.file_name().unwrap().to_string_lossy().into_owned();
        assert_eq!(names(explorer.rows(&[root.clone()])), vec![top.clone(), "  src".into(), "  b.txt".into()]);
        explorer.toggle(&root.join("src"), false);
        assert_eq!(names(explorer.rows(&[root.clone()])), vec![top, "  src".into(), "    main.rs".into(), "  b.txt".into()]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn el_arbol_de_code_oculta_node_modules_target_y_dist() {
        let root = std::env::temp_dir().join(format!("atic-explorer-hide-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for dir in ["src", ".git", "node_modules", "target", "dist", "distribucion"] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
        }
        let names = |explorer: &mut Explorer| explorer.rows(&[root.clone()]).into_iter().skip(1).map(|r| r.name).collect::<Vec<_>>();
        // El del Mando solo oculta `.git`; el de Atic Code, también lo generado.
        assert_eq!(names(&mut Explorer::default()), vec!["dist", "distribucion", "node_modules", "src", "target"]);
        assert_eq!(names(&mut Explorer::for_code()), vec!["distribucion", "src"]);
        assert!(is_hidden("target", true) && !is_hidden("target", false));
        assert!(is_hidden(".git", false));
        let _ = std::fs::remove_dir_all(&root);
    }
}
