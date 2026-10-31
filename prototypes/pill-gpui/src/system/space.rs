//! Liberar espacio: qué ocupa el disco y se puede volver a generar.
//!
//! Lo que se ofrece son compilaciones de proyectos (`target`, `node_modules`,
//! `build` de Gradle), cachés de herramientas (Cargo fuera del proyecto,
//! Gradle, npm, Homebrew…), cachés de apps y lo que deja Xcode, más los
//! instaladores olvidados en Descargas. Nada de documentos ni de sistema.
//!
//! Las reglas de seguridad, como las de Mole (`tw93/mole`), que es GPL y
//! solo sirvió de referencia:
//! - Lo usado en los últimos 7 días va desmarcado: se recompila, pero cuesta.
//! - Lo que está en uso (Gradle con su daemon, la caché de una app abierta,
//!   la carpeta desde donde corre Atic) no se puede elegir.
//! - Los instaladores son del usuario: van desmarcados.
//! - Todo va a la Papelera (se puede recuperar). El espacio se libera al
//!   vaciarla, y eso lo confirma el Finder.
//!
//! Por ahora solo en macOS: en Windows faltan las rutas y mandar a la
//! Papelera de reciclaje.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

/// Lo que no se muestra: no vale la pena.
const MIN_SIZE: u64 = 20 << 20;
/// Usado hace menos que esto: desmarcado.
const RECENT: Duration = Duration::from_secs(7 * 86_400);
/// Hasta qué profundidad se buscan proyectos dentro de cada carpeta.
const PROJECT_DEPTH: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Builds,
    Tools,
    Apps,
    Xcode,
    Installers,
}

impl Kind {
    /// Lo que se revisa solo. Los instaladores están en Descargas, que pide
    /// permiso: se revisan cuando el usuario lo pide (`scan_installers`).
    pub const AUTO: [Kind; 4] = [Kind::Builds, Kind::Tools, Kind::Apps, Kind::Xcode];
}

#[derive(Clone, Debug, PartialEq)]
pub enum Note {
    None,
    /// Hace cuántos segundos se usó por última vez.
    Used(u64),
    /// Lo está usando algo que corre ahora: no se puede elegir.
    InUse(String),
    /// Hay que volver a bajarlo o es del usuario.
    Keep,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub name: String,
    pub path: PathBuf,
    pub size: u64,
    pub note: Note,
    pub checked: bool,
}

impl Item {
    pub fn locked(&self) -> bool {
        matches!(self.note, Note::InUse(_))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    pub kind: Kind,
    pub items: Vec<Item>,
}

impl Group {
    /// Lo que se puede liberar (sin lo que está en uso).
    pub fn size(&self) -> u64 {
        self.items.iter().filter(|i| !i.locked()).map(|i| i.size).sum()
    }

    pub fn in_use(&self) -> usize {
        self.items.iter().filter(|i| i.locked()).count()
    }

    pub fn chosen(&self) -> u64 {
        self.items.iter().filter(|i| i.checked).map(|i| i.size).sum()
    }
}

/// El escaneo, que se llena de a un grupo mientras corre.
#[derive(Clone, Debug, Default)]
pub struct Scan {
    pub groups: Vec<Group>,
    /// El grupo que se está midiendo.
    pub current: Option<Kind>,
    pub done: bool,
}

/// Recorre todo, dejando cada grupo en `shared` apenas está listo.
pub fn scan(shared: Arc<Mutex<Scan>>) {
    let Some(home) = home() else {
        shared.lock().map(|mut s| s.done = true).ok();
        return;
    };
    let busy = Busy::now();
    let mut claimed: Vec<PathBuf> = Vec::new();
    for kind in Kind::AUTO {
        shared.lock().map(|mut s| s.current = Some(kind)).ok();
        let mut items = match kind {
            Kind::Builds => builds(&home),
            Kind::Tools => tools(&home, &busy),
            Kind::Apps => app_caches(&home, &busy, &claimed),
            Kind::Xcode => xcode(&home),
            Kind::Installers => Vec::new(),
        };
        items.retain(|i| i.size >= MIN_SIZE);
        for item in &mut items {
            // Desde aquí corre Atic: borrarlo lo dejaría sin sus archivos.
            if busy.exe.starts_with(&item.path) {
                item.note = Note::InUse("Atic".into());
            }
            if item.locked() {
                item.checked = false;
            }
        }
        items.sort_by(|a, b| b.size.cmp(&a.size));
        claimed.extend(items.iter().map(|i| i.path.clone()));
        if !items.is_empty() {
            shared.lock().map(|mut s| s.groups.push(Group { kind, items })).ok();
        }
    }
    shared
        .lock()
        .map(|mut s| {
            s.current = None;
            s.done = true;
        })
        .ok();
}

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from).filter(|h| h.is_dir())
}

/// Lo que corre ahora y bloquea borrar algo.
struct Busy {
    /// Nombres y claves de las apps abiertas, en minúsculas.
    apps: Vec<String>,
    gradle: bool,
    cargo: bool,
    exe: PathBuf,
}

impl Busy {
    fn now() -> Self {
        let ps = std::process::Command::new("ps")
            .args(["-axww", "-o", "comm="])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).to_lowercase())
            .unwrap_or_default();
        let mut apps = Vec::new();
        for line in ps.lines() {
            // «/Applications/Zen.app/Contents/MacOS/zen» → «zen».
            if let Some(end) = line.find(".app/") {
                if let Some(name) = line[..end].rsplit('/').next() {
                    apps.push(name.to_string());
                }
            }
            if let Some(exe) = line.rsplit('/').next() {
                apps.push(exe.to_string());
            }
        }
        apps.sort();
        apps.dedup();
        Self {
            gradle: ps.contains("gradledaemon") || ps.contains("kotlincompiledaemon") || ps.contains("gradle-daemon"),
            cargo: ps.lines().any(|l| l.ends_with("/cargo") || l.ends_with("/rustc") || l == "cargo" || l == "rustc"),
            apps,
            exe: std::env::current_exe().unwrap_or_default(),
        }
    }

    /// La app de una carpeta de caché (`com.spotify.client`, `Google`, `zen`)
    /// está abierta.
    fn app_open(&self, folder: &str) -> Option<String> {
        let folder = folder.to_lowercase();
        let last = folder.rsplit('.').next().unwrap_or(&folder).to_string();
        self.apps
            .iter()
            .find(|app| app.len() > 2 && (**app == folder || **app == last || folder.contains(app.as_str())))
            .cloned()
    }
}

// --- Medir -----------------------------------------------------------------------

/// Lo que ocupa en disco (bloques, no largo: un archivo con huecos cuenta lo
/// que de verdad usa). No sigue enlaces. Las subcarpetas se miden en paralelo.
pub fn size_of(path: &Path) -> u64 {
    use rayon::prelude::*;
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return 0;
    };
    if !meta.is_dir() {
        return allocated(&meta);
    }
    let Ok(entries) = std::fs::read_dir(path) else {
        return allocated(&meta);
    };
    let entries: Vec<_> = entries.flatten().map(|e| e.path()).collect();
    allocated(&meta) + entries.par_iter().map(|p| size_of(p)).sum::<u64>()
}

#[cfg(unix)]
fn allocated(meta: &std::fs::Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    meta.blocks() * 512
}

#[cfg(not(unix))]
fn allocated(meta: &std::fs::Metadata) -> u64 {
    meta.len()
}

fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::symlink_metadata(path).and_then(|m| m.modified()).ok()
}

fn ago(time: SystemTime) -> u64 {
    SystemTime::now().duration_since(time).map(|d| d.as_secs()).unwrap_or(0)
}

/// Un elemento con su tamaño; marcado salvo que se haya usado hace poco.
fn item(name: String, path: PathBuf, last_used: Option<SystemTime>) -> Item {
    let size = size_of(&path);
    let used = last_used.map(ago);
    let recent = used.is_some_and(|s| s < RECENT.as_secs());
    Item {
        name,
        path,
        size,
        note: used.map_or(Note::None, Note::Used),
        checked: !recent,
    }
}

// --- Compilaciones de proyectos ---------------------------------------------------

/// Las carpetas donde suele haber proyectos. Sin Escritorio, Documentos ni
/// Descargas: macOS pide permiso para entrar y el aviso saldría de la nada.
fn project_roots(home: &Path) -> Vec<PathBuf> {
    ["dev", "Developer", "Projects", "projects", "code", "Code", "src", "repos", "workspace", "GitHub"]
        .iter()
        .map(|r| home.join(r))
        .filter(|p| p.is_dir())
        .collect()
}

/// Lo que se regenera de un proyecto según sus archivos marcadores (la idea
/// de `kondo`): `(marcador, carpeta)`.
pub(crate) const ARTIFACTS: [(&str, &str); 8] = [
    ("Cargo.toml", "target"),
    ("package.json", "node_modules"),
    ("package.json", ".next"),
    ("build.gradle", "build"),
    ("build.gradle.kts", "build"),
    ("settings.gradle.kts", ".gradle"),
    ("settings.gradle", ".gradle"),
    ("Package.swift", ".build"),
];

/// Las carpetas regenerables de un proyecto.
pub(crate) fn artifacts_of(has: impl Fn(&str) -> bool) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = ARTIFACTS
        .iter()
        .filter(|(marker, artifact)| has(marker) && has(artifact))
        .map(|(_, artifact)| *artifact)
        .collect();
    out.dedup();
    out
}

/// Cuándo se tocó el proyecto: lo último entre git, sus archivos de arriba
/// y la compilación misma.
fn project_used(dir: &Path, artifact: &Path) -> Option<SystemTime> {
    let mut times: Vec<SystemTime> = [dir.join(".git/index"), dir.join(".git/HEAD"), artifact.to_path_buf()]
        .iter()
        .filter_map(|p| modified(p))
        .collect();
    for child in [dir.to_path_buf(), artifact.to_path_buf()] {
        if let Ok(entries) = std::fs::read_dir(&child) {
            times.extend(entries.flatten().take(64).filter_map(|e| e.metadata().and_then(|m| m.modified()).ok()));
        }
    }
    times.into_iter().max()
}

fn builds(home: &Path) -> Vec<Item> {
    let mut found: Vec<(PathBuf, &'static str)> = Vec::new();
    for root in project_roots(home) {
        walk_projects(&root, 0, &mut found);
    }
    found.sort();
    found.dedup();
    found
        .into_iter()
        .map(|(dir, artifact)| {
            let path = dir.join(artifact);
            let project = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let used = project_used(&dir, &path);
            item(format!("{project} · {artifact}"), path, used)
        })
        .collect()
}

fn walk_projects(dir: &Path, depth: usize, found: &mut Vec<(PathBuf, &'static str)>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let names: Vec<String> = entries.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    let has = |n: &str| names.iter().any(|x| x == n);
    let artifacts = artifacts_of(has);
    for artifact in &artifacts {
        found.push((dir.to_path_buf(), artifact));
    }
    if depth >= PROJECT_DEPTH {
        return;
    }
    for name in &names {
        // Ni lo oculto ni lo regenerable: ahí no hay proyectos propios.
        if name.starts_with('.') || artifacts.contains(&name.as_str()) || ["node_modules", "target", "build", "Library"].contains(&name.as_str()) {
            continue;
        }
        let path = dir.join(name);
        if std::fs::symlink_metadata(&path).is_ok_and(|m| m.is_dir()) {
            walk_projects(&path, depth + 1, found);
        }
    }
}

// --- Cachés de herramientas -----------------------------------------------------------

fn tools(home: &Path, busy: &Busy) -> Vec<Item> {
    let caches = home.join("Library/Caches");
    let mut out = Vec::new();
    // Carpetas de compilación de Cargo fuera del proyecto (las marca
    // `CACHEDIR.TAG`), como la de la pill.
    if let Ok(entries) = std::fs::read_dir(&caches) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.join("CACHEDIR.TAG").is_file() {
                let name = entry.file_name().to_string_lossy().into_owned();
                let used = modified(&path.join("debug")).or_else(|| modified(&path));
                let mut it = item(format!("{name} · Rust"), path, used);
                if busy.cargo {
                    it.note = Note::InUse("cargo".into());
                }
                out.push(it);
            }
        }
    }
    let known: [(&str, &str, Option<&str>); 11] = [
        (".gradle/caches", "Gradle", Some("gradle")),
        (".npm/_cacache", "npm", None),
        ("Library/Caches/pnpm", "pnpm", None),
        ("Library/pnpm/store", "pnpm · store", None),
        ("Library/Caches/Yarn", "Yarn", None),
        ("Library/Caches/pip", "pip", None),
        ("Library/Caches/Homebrew", "Homebrew", None),
        ("Library/Caches/ms-playwright", "Playwright", None),
        ("Library/Caches/Mozilla.sccache", "sccache", Some("cargo")),
        ("Library/Caches/CocoaPods", "CocoaPods", None),
        ("Library/Caches/go-build", "Go", None),
    ];
    for (rel, name, lock) in known {
        let path = home.join(rel);
        if !path.is_dir() {
            continue;
        }
        let mut it = item(name.to_string(), path, None);
        let in_use = match lock {
            Some("gradle") => busy.gradle,
            Some("cargo") => busy.cargo,
            _ => false,
        };
        if in_use {
            it.note = Note::InUse(name.to_lowercase());
        }
        out.push(it);
    }
    out
}

// --- Cachés de apps -------------------------------------------------------------------

fn app_caches(home: &Path, busy: &Busy, claimed: &[PathBuf]) -> Vec<Item> {
    let caches = home.join("Library/Caches");
    let Ok(entries) = std::fs::read_dir(&caches) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let folder = entry.file_name().to_string_lossy().into_owned();
        // Lo de Apple lo maneja el sistema (y mucho está protegido).
        if folder.starts_with("com.apple.") || folder.starts_with('.') || claimed.iter().any(|c| c.starts_with(&path) || path.starts_with(c)) {
            continue;
        }
        if !std::fs::symlink_metadata(&path).is_ok_and(|m| m.is_dir()) {
            continue;
        }
        let mut it = item(pretty_folder(&folder), path, None);
        if let Some(app) = busy.app_open(&folder) {
            it.note = Note::InUse(app);
        }
        out.push(it);
    }
    out
}

/// `com.spotify.client` → `Spotify`; `zen` → `Zen`.
pub(crate) fn pretty_folder(folder: &str) -> String {
    let parts: Vec<&str> = folder.split('.').collect();
    let name = if parts.len() >= 3 && parts[0].len() <= 3 {
        // Dominio al revés: el nombre es lo que sigue a la empresa.
        parts[2..].iter().find(|p| !["client", "app", "desktop", "helper", "mac", "macos"].contains(p)).unwrap_or(&parts[1])
    } else {
        &folder
    };
    let mut chars = name.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().chain(chars).collect(),
        None => folder.to_string(),
    }
}

// --- Xcode y Descargas ------------------------------------------------------------------

fn xcode(home: &Path) -> Vec<Item> {
    let dev = home.join("Library/Developer");
    let mut out = Vec::new();
    for (rel, name, keep) in [
        ("Xcode/DerivedData", "DerivedData", false),
        ("Xcode/iOS DeviceSupport", "iOS DeviceSupport", false),
        ("CoreSimulator/Caches", "Simulator · cachés", false),
        ("Xcode/Archives", "Archives", true),
    ] {
        let path = dev.join(rel);
        if path.is_dir() {
            let mut it = item(name.to_string(), path, None);
            // Las Archives sirven para leer los cierres de las apps publicadas.
            if keep {
                it.note = Note::Keep;
                it.checked = false;
            }
            out.push(it);
        }
    }
    out
}

/// Los instaladores de Descargas (la primera vez, macOS pide permiso).
pub fn scan_installers() -> Option<Group> {
    let mut items = installers(&home()?);
    items.retain(|i| i.size >= MIN_SIZE);
    items.sort_by(|a, b| b.size.cmp(&a.size));
    (!items.is_empty()).then_some(Group { kind: Kind::Installers, items })
}

fn installers(home: &Path) -> Vec<Item> {
    let Ok(entries) = std::fs::read_dir(home.join("Downloads")) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|e| {
            let name = e.file_name().to_string_lossy().to_lowercase();
            [".dmg", ".pkg", ".iso", ".xip"].iter().any(|ext| name.ends_with(ext))
        })
        .map(|e| {
            let path = e.path();
            let used = modified(&path);
            let mut it = item(e.file_name().to_string_lossy().into_owned(), path, used);
            it.checked = false;
            it
        })
        .collect()
}

// --- Papelera ---------------------------------------------------------------------------

/// Manda a la Papelera. Devuelve lo que se movió (bytes) y los errores.
pub fn trash(items: &[(PathBuf, u64)]) -> (u64, Vec<String>) {
    let mut moved = 0;
    let mut errors = Vec::new();
    for (path, size) in items {
        match trash_one(path) {
            Ok(()) => moved += size,
            Err(error) => errors.push(format!("{}: {error}", path.display())),
        }
    }
    (moved, errors)
}

#[cfg(target_os = "macos")]
fn trash_one(path: &Path) -> Result<(), String> {
    use objc2::rc::Retained;
    use objc2::runtime::{AnyObject, Bool};
    use objc2::{class, msg_send};
    use objc2_foundation::NSString;
    objc2::rc::autoreleasepool(|_| unsafe {
        let text = NSString::from_str(&path.to_string_lossy());
        let url: Retained<AnyObject> = msg_send![class!(NSURL), fileURLWithPath: &*text];
        let manager: Retained<AnyObject> = msg_send![class!(NSFileManager), defaultManager];
        let ok: Bool = msg_send![
            &manager,
            trashItemAtURL: &*url,
            resultingItemURL: std::ptr::null_mut::<*mut AnyObject>(),
            error: std::ptr::null_mut::<*mut AnyObject>()
        ];
        if ok.as_bool() {
            Ok(())
        } else {
            Err(crate::i18n::t("pill.system.space.trashFailed").to_string())
        }
    })
}

#[cfg(not(target_os = "macos"))]
fn trash_one(_: &Path) -> Result<(), String> {
    Err(crate::i18n::t("pill.system.space.trashFailed").to_string())
}

/// Lo que hay en la Papelera.
pub fn trash_size() -> u64 {
    home().map_or(0, |h| size_of(&h.join(".Trash")))
}

/// Vaciar la Papelera con el Finder, que pide confirmación como siempre.
pub fn empty_trash() -> Result<(), String> {
    crate::launcher::system_action(crate::launcher::Action::Trash)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reconoce_lo_regenerable_por_sus_marcadores() {
        let names = ["Cargo.toml", "target", "src", "package.json", "node_modules"];
        let has = |n: &str| names.contains(&n);
        assert_eq!(artifacts_of(has), ["target", "node_modules"]);
        // Sin el marcador, una carpeta `build` es del usuario.
        let names = ["build", "README.md"];
        let has = |n: &str| names.contains(&n);
        assert!(artifacts_of(has).is_empty());
    }

    #[test]
    fn nombres_de_carpetas_de_cache() {
        assert_eq!(pretty_folder("com.spotify.client"), "Spotify");
        assert_eq!(pretty_folder("zen"), "Zen");
        assert_eq!(pretty_folder("com.microsoft.VSCode"), "VSCode");
        assert_eq!(pretty_folder("Google"), "Google");
    }

    #[test]
    fn mide_lo_que_ocupa_una_carpeta() {
        let dir = std::env::temp_dir().join(format!("atic-space-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("a/b")).unwrap();
        std::fs::write(dir.join("a/b/f"), vec![1u8; 100_000]).unwrap();
        std::fs::write(dir.join("g"), vec![1u8; 50_000]).unwrap();
        let size = size_of(&dir);
        std::fs::remove_dir_all(&dir).ok();
        assert!(size >= 150_000, "{size}");
    }

    /// Manda un archivo de prueba a la Papelera de verdad (con
    /// `--ignored`): comprueba la llamada a `NSFileManager`.
    #[cfg(target_os = "macos")]
    #[test]
    #[ignore]
    fn manda_a_la_papelera() {
        let file = std::env::temp_dir().join(format!("atic-papelera-{}.txt", std::process::id()));
        std::fs::write(&file, b"prueba de Atic").unwrap();
        let (moved, errors) = trash(&[(file.clone(), 14)]);
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(moved, 14);
        assert!(!file.exists());
    }

    #[test]
    fn lo_usado_hace_poco_va_desmarcado() {
        let dir = std::env::temp_dir().join(format!("atic-space-recent-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let fresh = item("x".into(), dir.clone(), Some(SystemTime::now()));
        let old = item("y".into(), dir.clone(), Some(SystemTime::now() - Duration::from_secs(30 * 86_400)));
        std::fs::remove_dir_all(&dir).ok();
        assert!(!fresh.checked);
        assert!(old.checked);
        assert!(matches!(old.note, Note::Used(s) if s >= 29 * 86_400));
    }
}
