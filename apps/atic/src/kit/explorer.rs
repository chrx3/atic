//! El árbol de archivos de un proyecto, en el panel Archivos de Atic Code.
//!
//! Las carpetas se leen al abrirlas y se vuelven a leer si lo que se tiene
//! guardado es de hace más de unos segundos: los agentes crean y borran
//! archivos todo el tiempo. Las carpetas raíz parten abiertas; `.git` no se
//! muestra. El de Atic Code (`Explorer::for_code`) tampoco muestra lo que la referencia
//! oculta: `node_modules`, `target`, `dist` y `.DS_Store`.
//!
//! Además respeta los `.gitignore` (con el crate `ignore`, como el índice de
//! búsqueda), salvo con el interruptor «Mostrar ignorados». Si el árbol se
//! recorta (`MAX_ROWS`), `truncated` lo dice para avisar en el panel. Y con
//! `poll` se mira barato si cambió algo de lo abierto (el mtime de cada
//! carpeta), para re-leer solo eso.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant, SystemTime};

use ignore::gitignore::Gitignore;

/// Lo leído de una carpeta vale por este tiempo.
const FRESH_FOR: Duration = Duration::from_secs(5);
/// Lo que tarda en rebuscarse un `.gitignore` cambiado.
const MATCHER_TTL: Duration = Duration::from_secs(30);
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

/// Por qué una carpeta abierta no muestra nada: no tiene nada visible o no se
/// pudo leer. El panel muestra una nota en vez de nada.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DirNote {
    Empty,
    Denied,
}

/// Lo que le pasa a `dir`: si se lee, va vacía (o con todo oculto); si no, sin permiso.
pub fn dir_note(dir: &Path) -> DirNote {
    match std::fs::read_dir(dir) {
        Ok(_) => DirNote::Empty,
        Err(_) => DirNote::Denied,
    }
}

#[derive(Default)]
pub struct Explorer {
    /// Oculta también `node_modules`, `target` y `dist` (el árbol de Atic Code).
    hide_build: bool,
    /// Si también se ven los ignorados por los `.gitignore` (el interruptor del panel).
    show_ignored: bool,
    /// Lo que se abrió o cerró a mano. Lo demás: abiertas las raíces.
    expanded: HashMap<PathBuf, bool>,
    cache: HashMap<PathBuf, (Instant, Vec<Item>)>,
    /// El mtime de cada carpeta la última vez que se leyó (para `poll`).
    mtimes: HashMap<PathBuf, SystemTime>,
    /// Las carpetas abiertas que se dibujaron en el último `rows` (lo que mira `poll`).
    watched: Vec<PathBuf>,
    /// Si el último `rows` se recortó por `MAX_ROWS` (el panel avisa).
    truncated: bool,
    /// Los `.gitignore` ya leídos, por raíz (se rehacen cada `MATCHER_TTL`).
    matchers: HashMap<PathBuf, (Instant, Rc<Gitignore>)>,
}

fn mtime_of(dir: &Path) -> Option<SystemTime> {
    std::fs::metadata(dir).and_then(|meta| meta.modified()).ok()
}

fn read(dir: &Path, hide_build: bool, show_ignored: bool, matcher: Option<&Gitignore>) -> Vec<Item> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut items: Vec<Item> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            if is_hidden(&name, hide_build) {
                return None;
            }
            let dir = entry.file_type().is_ok_and(|t| t.is_dir());
            // Lo ignorado por un `.gitignore` no se ve, salvo con el interruptor.
            if !show_ignored && matcher.is_some_and(|m| m.matched(entry.path(), dir).is_ignore()) {
                return None;
            }
            Some(Item { dir, path: entry.path(), name })
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

    /// Tras renombrar `old` a `new`: lo abierto, lo leído y lo mirado la siguen.
    pub fn retarget(&mut self, old: &Path, new: &Path) {
        let moved = |path: &PathBuf| {
            if path.as_path() == old {
                Some(new.to_path_buf())
            } else {
                path.strip_prefix(old).ok().map(|rest| new.join(rest))
            }
        };
        let mut expanded = HashMap::new();
        for (path, open) in std::mem::take(&mut self.expanded) {
            expanded.insert(moved(&path).unwrap_or(path), open);
        }
        self.expanded = expanded;
        let mut cache = HashMap::new();
        for (path, entry) in std::mem::take(&mut self.cache) {
            cache.insert(moved(&path).unwrap_or(path), entry);
        }
        self.cache = cache;
        let mut mtimes = HashMap::new();
        for (path, mtime) in std::mem::take(&mut self.mtimes) {
            mtimes.insert(moved(&path).unwrap_or(path), mtime);
        }
        self.mtimes = mtimes;
        for watched in &mut self.watched {
            if let Some(dest) = moved(watched) {
                *watched = dest;
            }
        }
    }

    /// Si se ven los ignorados por los `.gitignore`: al cambiar se relee todo.
    pub fn showing_ignored(&self) -> bool {
        self.show_ignored
    }

    pub fn set_show_ignored(&mut self, show: bool) {
        if self.show_ignored != show {
            self.show_ignored = show;
            self.refresh();
        }
    }

    /// Las carpetas abiertas a mano (para guardarlas por espacio).
    pub fn open_paths(&self) -> Vec<PathBuf> {
        self.expanded.iter().filter(|(_, open)| **open).map(|(path, _)| path.clone()).collect()
    }

    /// Devuelve las carpetas que quedaron abiertas en otro momento.
    pub fn set_open_paths(&mut self, paths: &[PathBuf]) {
        for path in paths {
            self.expanded.insert(path.clone(), true);
        }
    }

    /// Si el último `rows` se recortó por `MAX_ROWS`.
    pub fn truncated(&self) -> bool {
        self.truncated
    }

    pub fn refresh(&mut self) {
        self.cache.clear();
        self.mtimes.clear();
        self.matchers.clear();
    }

    /// ¿Cambió algo de lo abierto desde que se leyó? Mira solo el mtime de esas
    /// carpetas: barato para sondear cada par de segundos. Lo que cambió se saca
    /// del caché para re-leerlo en el próximo `rows`.
    pub fn poll(&mut self) -> bool {
        let mut changed = false;
        for dir in std::mem::take(&mut self.watched) {
            let now = mtime_of(&dir);
            match (self.mtimes.get(&dir), now) {
                (Some(before), Some(now)) if now != *before => {
                    self.cache.remove(&dir);
                    changed = true;
                }
                // Apareció o desapareció: también se relee.
                (Some(_), None) | (None, Some(_)) if self.cache.contains_key(&dir) => {
                    self.cache.remove(&dir);
                    changed = true;
                }
                _ => {}
            }
            self.watched.push(dir);
        }
        changed
    }

    /// Los `.gitignore` que valen bajo `root` (el suyo y los de dentro), o nada
    /// si no hay ninguno. Se rehacen cada `MATCHER_TTL`.
    fn matcher_for(&mut self, root: &Path) -> Option<Rc<Gitignore>> {
        let now = Instant::now();
        if let Some((at, matcher)) = self.matchers.get(root) {
            if now.duration_since(*at) < MATCHER_TTL {
                return Some(matcher.clone());
            }
        }
        let found: Vec<PathBuf> = ignore::WalkBuilder::new(root)
            .hidden(false)
            .git_ignore(true)
            .require_git(false)
            .filter_entry(|entry| entry.file_name() != ".git")
            .build()
            .flatten()
            .take(4096)
            .filter(|entry| entry.file_name() == ".gitignore" && entry.file_type().is_some_and(|t| t.is_file()))
            .take(64)
            .map(|entry| entry.into_path())
            .collect();
        if found.is_empty() {
            return None;
        }
        let mut builder = ignore::gitignore::GitignoreBuilder::new(root);
        for path in &found {
            let _ = builder.add(path);
        }
        let matcher = Rc::new(builder.build().ok()?);
        self.matchers.insert(root.to_path_buf(), (now, matcher.clone()));
        Some(matcher)
    }

    fn children(&mut self, dir: &Path, now: Instant, matcher: Option<&Gitignore>) -> Vec<Item> {
        match self.cache.get(dir) {
            Some((at, items)) if now.duration_since(*at) < FRESH_FOR => items.clone(),
            _ => {
                let items = read(dir, self.hide_build, self.show_ignored, matcher);
                if let Some(mtime) = mtime_of(dir) {
                    self.mtimes.insert(dir.to_path_buf(), mtime);
                }
                self.cache.insert(dir.to_path_buf(), (now, items.clone()));
                items
            }
        }
    }

    /// Las filas a la vista, en orden, con su sangría.
    pub fn rows(&mut self, roots: &[PathBuf]) -> Vec<Row> {
        let now = Instant::now();
        self.truncated = false;
        self.watched.clear();
        let mut out = Vec::new();
        for root in roots {
            let matcher = (!self.show_ignored).then(|| self.matcher_for(root)).flatten();
            let name = root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| root.display().to_string());
            self.walk(root, name, true, 0, now, matcher.as_deref(), &mut out);
        }
        out
    }

    fn walk(&mut self, path: &Path, name: String, dir: bool, depth: usize, now: Instant, matcher: Option<&Gitignore>, out: &mut Vec<Row>) {
        if out.len() >= MAX_ROWS {
            self.truncated = true;
            return;
        }
        let open = dir && self.is_open(path, depth == 0);
        if open {
            self.watched.push(path.to_path_buf());
        }
        out.push(Row { depth, path: path.to_path_buf(), name, dir, open });
        if open {
            for item in self.children(path, now, matcher) {
                self.walk(&item.path, item.name, item.dir, depth + 1, now, matcher, out);
                if self.truncated {
                    return;
                }
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
    fn la_nota_dice_si_la_carpeta_esta_vacia_o_no_se_lee() {
        let root = std::env::temp_dir().join(format!("atic-explorer-note-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("vacia")).unwrap();
        assert_eq!(dir_note(&root.join("vacia")), DirNote::Empty);
        assert_eq!(dir_note(&root.join("no-existe")), DirNote::Denied);
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
        // El de omisión solo oculta `.git`; el de Atic Code, también lo generado.
        assert_eq!(names(&mut Explorer::default()), vec!["dist", "distribucion", "node_modules", "src", "target"]);
        assert_eq!(names(&mut Explorer::for_code()), vec!["distribucion", "src"]);
        assert!(is_hidden("target", true) && !is_hidden("target", false));
        assert!(is_hidden(".git", false));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn el_arbol_respeta_gitignore_salvo_con_el_interruptor() {
        let root = std::env::temp_dir().join(format!("atic-explorer-ignore-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join("gen")).unwrap();
        std::fs::write(root.join(".gitignore"), "gen/\n*.log\n").unwrap();
        std::fs::write(root.join("src/main.rs"), "x").unwrap();
        std::fs::write(root.join("gen/salida.js"), "x").unwrap();
        std::fs::write(root.join("debug.log"), "x").unwrap();
        let names = |explorer: &mut Explorer| explorer.rows(&[root.clone()]).into_iter().skip(1).map(|r| r.name).collect::<Vec<_>>();
        let mut explorer = Explorer::for_code();
        assert!(!explorer.showing_ignored());
        assert_eq!(names(&mut explorer), vec!["src"]);
        explorer.set_show_ignored(true);
        assert!(explorer.showing_ignored());
        // Orden: carpetas primero, luego archivos por nombre.
        assert_eq!(names(&mut explorer), vec!["gen", "src", "debug.log"]);
        explorer.set_show_ignored(false);
        assert_eq!(names(&mut explorer), vec!["src"]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn lo_abierto_se_guarda_y_se_devuelve() {
        let mut explorer = Explorer::for_code();
        let a = PathBuf::from("/tmp/e/src");
        let b = PathBuf::from("/tmp/e/web");
        explorer.set_open_paths(&[a.clone(), b.clone()]);
        let mut open = explorer.open_paths();
        open.sort();
        assert_eq!(open, vec![a, b]);
    }

    #[test]
    fn el_sondeo_nota_lo_que_cambio_en_lo_abierto() {
        let root = std::env::temp_dir().join(format!("atic-explorer-poll-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/a.rs"), "x").unwrap();
        let mut explorer = Explorer::for_code();
        explorer.rows(&[root.clone()]);
        // Nada cambió: no hay nada que releer.
        assert!(!explorer.poll());
        // Un archivo nuevo cambia el mtime de su carpeta: se relee en el próximo cuadro.
        std::thread::sleep(std::time::Duration::from_millis(10));
        std::fs::write(root.join("src/b.rs"), "x").unwrap();
        assert!(explorer.poll());
        let names: Vec<String> = explorer.rows(&[root.clone()]).into_iter().skip(2).map(|r| r.name).collect();
        assert_eq!(names, vec!["a.rs", "b.rs"]);
        assert!(!explorer.poll());
        let _ = std::fs::remove_dir_all(&root);
    }
}
