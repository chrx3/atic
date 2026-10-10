//! El índice de archivos de un espacio, como `fs_index` de la referencia: todas las
//! carpetas recorridas respetando `.gitignore` (también fuera de un repo),
//! sin `.git`. Lo usa la búsqueda del panel Archivos y lo usarán las
//! @-menciones. Recorrer un repo grande tarda: va en segundo plano.

use std::path::{Path, PathBuf};

/// Como mucho tantos archivos (el límite de la referencia).
pub const MAX_INDEX: usize = 60_000;
/// Resultados que muestra la búsqueda del panel.
pub const MAX_RESULTS: usize = 120;

#[derive(Clone, Debug, PartialEq)]
pub struct Indexed {
    pub path: PathBuf,
    /// La ruta dentro de su carpeta, con `/`.
    pub rel: String,
    /// El nombre de su carpeta raíz (para distinguir entre varias).
    pub root: String,
}

impl Indexed {
    pub fn name(&self) -> &str {
        self.rel.rsplit('/').next().unwrap_or(&self.rel)
    }

    /// La carpeta dentro del espacio («raíz/sub» con varias raíces).
    pub fn dir(&self, with_root: bool) -> String {
        let parent = self.rel.rsplit_once('/').map(|(dir, _)| dir).unwrap_or("");
        match (with_root, parent.is_empty()) {
            (true, true) => self.root.clone(),
            (true, false) => format!("{}/{parent}", self.root),
            (false, _) => parent.to_string(),
        }
    }
}

/// Los archivos de estas carpetas.
pub fn index(roots: &[PathBuf]) -> Vec<Indexed> {
    let mut out = Vec::new();
    for root in roots {
        let name = crate::space::workspaces::short_name(root);
        let walker = ignore::WalkBuilder::new(root)
            .hidden(false)
            .git_ignore(true)
            .require_git(false)
            .filter_entry(|entry| entry.file_name() != ".git")
            .build();
        for entry in walker.flatten() {
            if out.len() >= MAX_INDEX {
                return out;
            }
            if !entry.file_type().is_some_and(|t| t.is_file()) {
                continue;
            }
            let rel = relative(entry.path(), root);
            out.push(Indexed { path: entry.path().to_path_buf(), rel, root: name.clone() });
        }
    }
    out
}

fn relative(path: &Path, root: &Path) -> String {
    path.strip_prefix(root).map(|r| r.to_string_lossy().replace('\\', "/")).unwrap_or_default()
}

/// Los que contienen `query` en su ruta: primero los que la tienen en el
/// nombre y, entre iguales, las rutas más cortas (como el panel de la referencia).
pub fn search<'a>(files: &'a [Indexed], query: &str, limit: usize) -> Vec<&'a Indexed> {
    let needle = query.trim().to_lowercase();
    if needle.is_empty() {
        return Vec::new();
    }
    let mut found: Vec<(bool, usize, &Indexed)> = files
        .iter()
        .filter(|f| f.rel.to_lowercase().contains(&needle))
        .map(|f| (!f.name().to_lowercase().contains(&needle), f.rel.len(), f))
        .collect();
    found.sort_by(|a, b| (a.0, a.1).cmp(&(b.0, b.1)));
    found.into_iter().take(limit).map(|(_, _, f)| f).collect()
}

/// El archivo al que apunta un `código` en línea del chat (`src/x.rs`, `x.rs:42`,
/// `C:\repo\x.rs`): absoluta, o relativa a alguna de las carpetas del espacio;
/// con varias carpetas también vale `carpeta/ruta`, como las @-menciones. `None`
/// si no es un archivo que exista.
pub fn resolve_ref(text: &str, roots: &[PathBuf]) -> Option<PathBuf> {
    let (path, _) = gpui_m3::split_path_line(text.trim());
    let path = Path::new(path);
    if path.is_absolute() {
        return path.is_file().then(|| path.to_path_buf());
    }
    let first = path.components().next().map(|c| c.as_os_str().to_string_lossy().into_owned());
    for root in roots {
        let direct = root.join(path);
        if direct.is_file() {
            return Some(direct);
        }
        let named = crate::space::workspaces::short_name(root);
        if let (Some(first), true) = (&first, roots.len() > 1) {
            if *first == named {
                let rest = root.join(path.strip_prefix(first).unwrap_or(path));
                if rest.is_file() {
                    return Some(rest);
                }
            }
        }
    }
    None
}

/// Un enlace del markdown solo se abre si es web o correo: nada de `file:` ni de
/// esquemas de otros programas.
pub fn is_safe_link(url: &str) -> bool {
    let lower = url.trim().to_ascii_lowercase();
    ["http://", "https://", "mailto:"].iter().any(|scheme| lower.starts_with(scheme))
}

/// El tipo de un archivo, para el color de su ícono (`kindOf` de la referencia).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Code,
    Style,
    Doc,
    Config,
    Image,
    Other,
}

pub fn kind_of(name: &str) -> Kind {
    let ext = name.rsplit_once('.').map(|(_, ext)| ext.to_lowercase()).unwrap_or_default();
    match ext.as_str() {
        "ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs" | "py" | "rs" | "go" | "java" | "kt" | "swift" | "c" | "h" | "cpp" | "cs" | "rb" | "php" | "sh"
        | "sql" | "vue" | "svelte" => Kind::Code,
        "css" | "scss" | "sass" | "less" | "html" => Kind::Style,
        "md" | "mdx" | "txt" | "rst" | "pdf" => Kind::Doc,
        "json" | "jsonc" | "yaml" | "yml" | "toml" | "ini" | "env" | "lock" | "xml" | "cfg" | "conf" => Kind::Config,
        "png" | "jpg" | "jpeg" | "gif" | "svg" | "webp" | "ico" => Kind::Image,
        _ if name.starts_with(".env") => Kind::Config,
        _ => Kind::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("atic-index-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn el_indice_respeta_gitignore_y_salta_git() {
        let root = temp("ignore");
        for dir in ["src", "target/debug", ".git", "node_modules/x", ".github"] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
        }
        std::fs::write(root.join(".gitignore"), "target/\nnode_modules/\n*.log\n").unwrap();
        for file in ["src/main.rs", "target/debug/app.exe", ".git/HEAD", "node_modules/x/i.js", "debug.log", ".github/ci.yml", "README.md"] {
            std::fs::write(root.join(file), "x").unwrap();
        }
        let mut rels: Vec<String> = index(&[root.clone()]).into_iter().map(|f| f.rel).collect();
        rels.sort();
        // Los ocultos se ven (.github, .gitignore); lo ignorado y .git, no.
        assert_eq!(rels, vec![".github/ci.yml", ".gitignore", "README.md", "src/main.rs"]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn la_busqueda_prefiere_el_nombre_y_las_rutas_cortas() {
        let file = |rel: &str| Indexed { path: PathBuf::from(rel), rel: rel.to_string(), root: "r".into() };
        let files = vec![file("docs/view/notes.md"), file("src/code/view.rs"), file("src/view.rs"), file("src/main.rs")];
        let found: Vec<&str> = search(&files, "View", 10).into_iter().map(|f| f.rel.as_str()).collect();
        assert_eq!(found, vec!["src/view.rs", "src/code/view.rs", "docs/view/notes.md"]);
        assert_eq!(search(&files, "view", 1).len(), 1);
        assert!(search(&files, "  ", 10).is_empty());
        assert_eq!(files[1].dir(true), "r/src/code");
        assert_eq!(files[2].name(), "view.rs");
    }

    #[test]
    fn el_tipo_sale_de_la_extension() {
        assert_eq!(kind_of("main.RS"), Kind::Code);
        assert_eq!(kind_of(".env.local"), Kind::Config);
        assert_eq!(kind_of("logo.svg"), Kind::Image);
        assert_eq!(kind_of("Makefile"), Kind::Other);
    }

    #[test]
    fn una_ruta_en_linea_se_resuelve_contra_las_carpetas_del_espacio() {
        let a = temp("ref-a");
        let b = temp("ref-b");
        std::fs::create_dir_all(a.join("src")).unwrap();
        std::fs::create_dir_all(b.join("lib")).unwrap();
        std::fs::write(a.join("src/main.rs"), "x").unwrap();
        std::fs::write(b.join("lib/util.rs"), "x").unwrap();
        let roots = vec![a.clone(), b.clone()];
        // Relativa, con o sin la línea.
        assert_eq!(resolve_ref("src/main.rs", &roots), Some(a.join("src/main.rs")));
        assert_eq!(resolve_ref("src/main.rs:42", &roots), Some(a.join("src/main.rs")));
        assert_eq!(resolve_ref("lib/util.rs:3-9", &roots), Some(b.join("lib/util.rs")));
        // Con la carpeta por delante.
        let named = crate::space::workspaces::short_name(&b);
        assert_eq!(resolve_ref(&format!("{named}/lib/util.rs"), &roots), Some(b.join("lib/util.rs")));
        // Absoluta.
        assert_eq!(resolve_ref(&a.join("src/main.rs").to_string_lossy(), &roots), Some(a.join("src/main.rs")));
        // Lo que no existe, o es una carpeta, no se abre.
        assert_eq!(resolve_ref("src/nada.rs", &roots), None);
        assert_eq!(resolve_ref("src", &roots), None);
        assert_eq!(resolve_ref("src/main.rs", &[]), None);
        let _ = std::fs::remove_dir_all(&a);
        let _ = std::fs::remove_dir_all(&b);
    }

    #[test]
    fn el_codigo_en_linea_que_parece_ruta_es_clicable() {
        // Lo que gpui-m3 vuelve clicable en una respuesta de Claude.
        let blocks = gpui_m3::parse_markdown("Mira `src/code/view.rs`, `mod.rs:42`, `cargo check` y `foo.bar()`.");
        let gpui_m3::MarkdownBlock::Paragraph(rich) = &blocks[0] else { panic!("no es un párrafo") };
        let clickable: Vec<&str> = rich.code_ranges().map(|r| &rich.text[r]).filter(|t| gpui_m3::looks_like_path(t)).collect();
        assert_eq!(clickable, vec!["src/code/view.rs", "mod.rs:42"]);
        assert!(gpui_m3::looks_like_path(r"C:\repo\src\main.rs:3-9"));
        assert_eq!(gpui_m3::split_path_line("mod.rs:42-50"), ("mod.rs", Some((42, Some(50)))));
    }

    #[test]
    fn solo_se_abren_enlaces_web_o_de_correo() {
        assert!(is_safe_link("https://example.com/a?b=1"));
        assert!(is_safe_link(" HTTP://example.com"));
        assert!(is_safe_link("mailto:a@b.cl"));
        assert!(!is_safe_link("file:///C:/Windows/System32/calc.exe"));
        assert!(!is_safe_link("ms-settings:network"));
        assert!(!is_safe_link("javascript:alert(1)"));
        assert!(!is_safe_link("src/main.rs"));
    }
}
