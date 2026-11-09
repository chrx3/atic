//! «Liberar espacio» en Windows: las rutas de cada grupo y la Papelera de reciclaje. Las
//! reglas son las de `space.rs`.
//!
//! A diferencia de macOS, las cachés de herramientas no se reconocen por el nombre de su
//! carpeta: en `%LOCALAPPDATA%` también viven instalaciones (`Android\Sdk`,
//! `JetBrains\Toolbox`, `Programs\…`) que una regla por nombre tomaría por caché. Solo se
//! ofrecen rutas conocidas.

use std::path::{Path, PathBuf};

use super::space::{item, modified, Item, Note, RECENT};

/// `%LOCALAPPDATA%` y `%APPDATA%`.
fn local() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from).filter(|p| p.is_dir())
}

fn roaming() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(PathBuf::from).filter(|p| p.is_dir())
}

/// La carpeta del usuario.
pub(super) fn home() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE").map(PathBuf::from).filter(|h| h.is_dir())
}

/// Los ejecutables que corren ahora, en minúsculas y sin `.exe`.
pub(super) fn running() -> Vec<String> {
    let mut names: Vec<String> = crate::agents::process_snapshot().into_iter().map(|(_, _, name)| name.trim_end_matches(".exe").to_string()).collect();
    names.sort();
    names.dedup();
    names
}

/// Donde suele haber proyectos: las de siempre en la carpeta del usuario, `source\repos` de
/// Visual Studio, los Documentos (también los de OneDrive) y `C:\dev`.
pub(super) fn project_roots(home: &Path) -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = ["dev", "Developer", "Projects", "projects", "code", "Code", "src", "repos", "source\\repos", "workspace", "GitHub", "Documents", "Documentos"]
        .iter()
        .map(|r| home.join(r))
        .collect();
    for var in ["OneDriveCommercial", "OneDriveConsumer", "OneDrive"] {
        if let Some(drive) = std::env::var_os(var).map(PathBuf::from) {
            roots.extend(["Documents", "Documentos", "GitHub"].iter().map(|r| drive.join(r)));
        }
    }
    roots.push(PathBuf::from("C:\\dev"));
    roots.retain(|p| p.is_dir());
    roots.sort();
    roots.dedup();
    roots
}

/// Las cachés de herramientas de desarrollo: rutas conocidas (`(base, ruta, nombre, bloqueo)`,
/// con `base` `home` o `local`) y las carpetas de compilación de Cargo fuera del proyecto (las
/// marca `CACHEDIR.TAG`), como la de la pill de desarrollo.
pub(super) fn tools(home: &Path, cargo_busy: bool, gradle_busy: bool) -> Vec<Item> {
    let mut out = Vec::new();
    let local = local();
    if let Some(local) = &local {
        if let Ok(entries) = std::fs::read_dir(local) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.join("CACHEDIR.TAG").is_file() {
                    let name = entry.file_name().to_string_lossy().into_owned();
                    let used = modified(&path.join("debug")).or_else(|| modified(&path));
                    let mut it = item(format!("{name} · Rust"), path, used);
                    if cargo_busy {
                        it.note = Note::InUse("cargo".into());
                        it.checked = false;
                    }
                    out.push(it);
                }
            }
        }
    }
    let known: [(&str, &str, &str, Option<&str>); 13] = [
        ("home", ".gradle\\caches", "Gradle", Some("gradle")),
        ("home", ".cargo\\registry\\cache", "Cargo · registro", Some("cargo")),
        ("home", ".nuget\\packages", "NuGet", None),
        ("local", "npm-cache", "npm", None),
        ("local", "pnpm\\store", "pnpm · store", None),
        ("local", "pnpm-cache", "pnpm", None),
        ("local", "Yarn\\Cache", "Yarn", None),
        ("local", "pip\\cache", "pip", None),
        ("local", "ms-playwright", "Playwright", None),
        ("local", "Mozilla\\sccache", "sccache", Some("cargo")),
        ("local", "go-build", "Go", None),
        ("local", "NuGet\\v3-cache", "NuGet · caché", None),
        ("local", "node-gyp\\Cache", "node-gyp", None),
    ];
    for (base, rel, name, lock) in known {
        let root = if base == "home" { Some(home.to_path_buf()) } else { local.clone() };
        let Some(path) = root.map(|r| r.join(rel)).filter(|p| p.is_dir()) else {
            continue;
        };
        let mut it = item(name.to_string(), path, None);
        let in_use = match lock {
            Some("gradle") => gradle_busy,
            Some("cargo") => cargo_busy,
            _ => false,
        };
        it.note = if in_use { Note::InUse(name.to_lowercase()) } else { Note::Redownload };
        it.checked = false;
        out.push(it);
    }
    out
}

/// Las cachés de apps: las de los navegadores Chromium (por perfil) y las de las apps de
/// Electron. Solo sus carpetas de caché, nunca los datos. `(base, carpeta, nombre, proceso)`.
pub(super) fn app_caches(running: &[String]) -> Vec<Item> {
    let mut out = Vec::new();
    let open = |process: &str| running.iter().any(|p| p == process);
    let (local, roaming) = (local(), roaming());
    let browsers: [(&str, &str, &str); 4] = [
        ("Google\\Chrome\\User Data", "Chrome", "chrome"),
        ("Microsoft\\Edge\\User Data", "Edge", "msedge"),
        ("BraveSoftware\\Brave-Browser\\User Data", "Brave", "brave"),
        ("Vivaldi\\User Data", "Vivaldi", "vivaldi"),
    ];
    for (rel, name, process) in browsers {
        let Some(data) = local.as_ref().map(|l| l.join(rel)).filter(|p| p.is_dir()) else {
            continue;
        };
        for profile in chromium_profiles(&data) {
            let label = profile.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            for cache in ["Cache", "Code Cache", "GPUCache", "Service Worker\\CacheStorage"] {
                let path = profile.join(cache);
                if path.is_dir() {
                    let mut it = item(format!("{name} · {label} · {cache}"), path, None);
                    if open(process) {
                        it.note = Note::InUse(name.to_string());
                        it.checked = false;
                    }
                    out.push(it);
                }
            }
        }
    }
    let electron: [(&str, &str, &str); 7] = [
        ("Code", "VS Code", "code"),
        ("Cursor", "Cursor", "cursor"),
        ("discord", "Discord", "discord"),
        ("Slack", "Slack", "slack"),
        ("Microsoft\\Teams", "Teams", "teams"),
        ("Notion", "Notion", "notion"),
        ("Postman", "Postman", "postman"),
    ];
    for (rel, name, process) in electron {
        let Some(base) = roaming.as_ref().map(|r| r.join(rel)).filter(|p| p.is_dir()) else {
            continue;
        };
        for cache in ["Cache", "Code Cache", "GPUCache", "CachedData"] {
            let path = base.join(cache);
            if path.is_dir() {
                let mut it = item(format!("{name} · {cache}"), path, None);
                if open(process) {
                    it.note = Note::InUse(name.to_string());
                    it.checked = false;
                }
                out.push(it);
            }
        }
    }
    if let Some(path) = local.as_ref().map(|l| l.join("Spotify\\Data")).filter(|p| p.is_dir()) {
        let mut it = item("Spotify".into(), path, None);
        if open("spotify") {
            it.note = Note::InUse("Spotify".into());
            it.checked = false;
        }
        out.push(it);
    }
    out
}

/// Los perfiles de un navegador Chromium: `Default` y `Profile N`.
fn chromium_profiles(data: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(data) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            name == "Default" || name.starts_with("Profile ")
        })
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect()
}

/// Lo de Windows que se acumula: lo viejo de la carpeta temporal y los volcados de cierres.
pub(super) fn system(running_exe: &Path) -> Vec<Item> {
    let mut out = Vec::new();
    if let Some(temp) = std::env::var_os("TEMP").map(PathBuf::from).filter(|p| p.is_dir()) {
        out.extend(old_temp_entries(&temp, running_exe));
    }
    if let Some(dumps) = local().map(|l| l.join("CrashDumps")).filter(|p| p.is_dir()) {
        out.push(item(crate::i18n::t("pill.system.space.crashDumps").to_string(), dumps, None));
    }
    out
}

/// Lo de primer nivel de la carpeta temporal sin tocar hace más de `RECENT` (la carpeta misma
/// no se mueve: las apps cuentan con que exista). Lo de hace poco puede estar en uso.
fn old_temp_entries(temp: &Path, running_exe: &Path) -> Vec<Item> {
    let Ok(entries) = std::fs::read_dir(temp) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            let used = modified(&path)?;
            let old = super::space::ago(used) >= RECENT.as_secs();
            (old && !running_exe.starts_with(&path)).then(|| {
                let name = format!("Temp · {}", e.file_name().to_string_lossy());
                item(name, path, Some(used))
            })
        })
        .collect()
}

/// Las extensiones de un instalador en Descargas.
pub(super) const INSTALLERS: [&str; 5] = [".exe", ".msi", ".msix", ".iso", ".appx"];

/// Manda a la Papelera de reciclaje (se puede deshacer). Si no cabe en ella, Windows avisa
/// antes de borrarlo para siempre (`FOF_WANTNUKEWARNING`).
pub(super) fn trash_one(path: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::Shell::{SHFileOperationW, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT, FOF_WANTNUKEWARNING, FO_DELETE, SHFILEOPSTRUCTW};
    // Una lista de rutas terminada en doble nulo.
    let mut from: Vec<u16> = path.as_os_str().encode_wide().collect();
    from.extend([0, 0]);
    let mut op: SHFILEOPSTRUCTW = unsafe { std::mem::zeroed() };
    op.wFunc = FO_DELETE as _;
    op.pFrom = from.as_ptr();
    op.fFlags = (FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_NOERRORUI | FOF_SILENT | FOF_WANTNUKEWARNING) as _;
    let code = unsafe { SHFileOperationW(&mut op) };
    if code == 0 && op.fAnyOperationsAborted == 0 {
        Ok(())
    } else {
        Err(crate::i18n::t("pill.system.space.trashFailed").to_string())
    }
}

/// Lo que hay en la Papelera de reciclaje (de todas las unidades).
pub(super) fn trash_size() -> u64 {
    use windows_sys::Win32::UI::Shell::{SHQueryRecycleBinW, SHQUERYRBINFO};
    let mut info: SHQUERYRBINFO = unsafe { std::mem::zeroed() };
    info.cbSize = std::mem::size_of::<SHQUERYRBINFO>() as u32;
    let ok = unsafe { SHQueryRecycleBinW(std::ptr::null(), &mut info) } >= 0;
    let size = { info.i64Size };
    if ok { size.max(0) as u64 } else { 0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lo_temporal_reciente_no_se_ofrece() {
        let dir = std::env::temp_dir().join(format!("atic-space-temp-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("viejo")).unwrap();
        std::fs::create_dir_all(dir.join("nuevo")).unwrap();
        let old = std::time::SystemTime::now() - std::time::Duration::from_secs(30 * 86_400);
        filetime_set(&dir.join("viejo"), old);
        let found = old_temp_entries(&dir, Path::new("C:\\nada\\pill.exe"));
        // Lo que corre desde ahí tampoco.
        let running = old_temp_entries(&dir, &dir.join("viejo").join("pill.exe"));
        std::fs::remove_dir_all(&dir).ok();
        assert_eq!(found.iter().map(|i| i.name.as_str()).collect::<Vec<_>>(), ["Temp · viejo"]);
        assert!(running.is_empty());
    }

    /// Cambia la fecha de modificación de una carpeta (para la prueba). Abrir una carpeta en
    /// Windows pide `FILE_FLAG_BACKUP_SEMANTICS`.
    fn filetime_set(path: &Path, time: std::time::SystemTime) {
        use std::os::windows::fs::OpenOptionsExt;
        let file = std::fs::File::options().write(true).custom_flags(0x0200_0000).open(path).unwrap();
        file.set_modified(time).unwrap();
    }

    #[test]
    fn los_perfiles_de_un_navegador() {
        let dir = std::env::temp_dir().join(format!("atic-space-chromium-{}", std::process::id()));
        for name in ["Default", "Profile 1", "System Profile", "Crashpad"] {
            std::fs::create_dir_all(dir.join(name)).unwrap();
        }
        let mut found: Vec<String> = chromium_profiles(&dir).iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect();
        found.sort();
        std::fs::remove_dir_all(&dir).ok();
        assert_eq!(found, ["Default", "Profile 1"]);
    }

    /// Manda un archivo de prueba a la Papelera de reciclaje de verdad (con `--ignored`).
    #[test]
    #[ignore]
    fn manda_a_la_papelera_de_reciclaje() {
        let file = std::env::temp_dir().join(format!("atic-papelera-{}.txt", std::process::id()));
        std::fs::write(&file, b"prueba de Atic").unwrap();
        trash_one(&file).unwrap();
        assert!(!file.exists());
    }
}
