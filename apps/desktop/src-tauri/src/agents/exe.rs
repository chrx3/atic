//! Encontrar el ejecutable de un agente.
//!
//! # Por qué esto existe
//!
//! `Command::new("opencode")` falla en Windows con «program not found», y
//! `Command::new("claude")` funciona. La diferencia no es el PATH: es que
//! `claude` se instala como `claude.exe` y `opencode` como un **shim de npm**
//! —`opencode.cmd` y `opencode.ps1`, sin `.exe` al lado—. Windows resuelve eso
//! consultando `PATHEXT`, pero `CreateProcess` (y por lo tanto `Command`) solo
//! agrega `.exe` por su cuenta. El que consulta `PATHEXT` es el intérprete de
//! comandos, no el sistema.
//!
//! Comprobado en esta máquina:
//!
//! ```text
//! claude   -> C:\Users\…\.local\bin\claude.exe          (anda solo)
//! codex    -> C:\…\Programs\OpenAI\Codex\bin\codex.exe   (anda solo)
//! opencode -> C:\Users\…\AppData\Roaming\npm\opencode.cmd (no lo encuentra)
//! ```
//!
//! # Qué NO se acepta
//!
//! `.ps1` y `.vbs` aparecen en `PATHEXT` y **no** son lanzables por
//! `CreateProcess`: necesitan que alguien los interprete. Devolverlos daría el
//! mismo «program not found» un paso más tarde, así que se filtran acá y no se
//! descubre el problema al arrancar la sesión.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// Extensiones que `Command` sabe lanzar en Windows.
///
/// `.bat` y `.cmd` van porque Rust los enruta por el intérprete desde 1.77,
/// con el escapado que arregló BatBadBut. El orden importa: un `.exe` real
/// siempre gana sobre el shim que lo envuelve.
#[cfg(windows)]
const LANZABLES: [&str; 4] = [".exe", ".com", ".cmd", ".bat"];

/// La ruta con la que lanzar `program`, o `None` si no está en el PATH.
///
/// Un nombre que ya trae separador se toma como ruta y no se busca: quien lo
/// escribió sabía dónde estaba.
pub fn resolve(program: &str) -> Option<PathBuf> {
    resolve_with(
        program,
        &search_dirs(),
        &exts_from_env(),
        SIN_EXTENSION_SIRVE,
        |p| p.is_file(),
    )
}

/// Dónde buscar: el PATH del proceso y, en Windows, el fresco del registro.
///
/// El env del proceso queda congelado al arrancar; un instalador que agrega
/// su carpeta al PATH del registro después (agy, claude, grok…) no se vería
/// hasta reiniciar Atic. Se relee por llamada: `cli_on_path` corre poco.
fn search_dirs() -> Vec<PathBuf> {
    #[cfg(windows)]
    {
        merge_path_dirs(dirs_from_env(), dirs_from_registry())
    }
    #[cfg(not(windows))]
    {
        dirs_from_env()
    }
}

/// El PATH fusionado como valor de variable, para el env de las consolas PTY.
/// `None` = nada que fusionar: el hijo hereda el PATH tal cual.
pub fn merged_path_var() -> Option<std::ffi::OsString> {
    #[cfg(windows)]
    {
        std::env::join_paths(search_dirs()).ok()
    }
    #[cfg(not(windows))]
    {
        None
    }
}

/// ¿Un archivo sin extensión puede ejecutarse?
///
/// En Unix sí, y es lo normal. En Windows **no**, y esa diferencia importa
/// más de lo que parece: npm instala TRES archivos al lado —`opencode`
/// (guion de shell, sin extensión), `opencode.cmd` y `opencode.ps1`—. Aceptar
/// el primero devuelve una ruta que existe, no es ejecutable, y falla recién
/// al lanzarla con el mismo error 193 que veníamos persiguiendo.
#[cfg(windows)]
const SIN_EXTENSION_SIRVE: bool = false;
#[cfg(not(windows))]
const SIN_EXTENSION_SIRVE: bool = true;

/// Con qué lanzar un agente: el ejecutable y los argumentos a anteponer.
///
/// # Por qué no alcanza con la ruta
///
/// Un `.cmd` **no es un ejecutable**: es un guion para el intérprete. Pasárselo
/// a `CreateProcess` devuelve «%1 is not a valid Win32 application» (error 193),
/// que es lo que pasó al lanzar `opencode.cmd` desde el crate de ACP. La
/// biblioteca estándar de Rust disimula esto desde 1.77 —detecta la extensión y
/// enruta por el intérprete—, pero `async-process`, que es lo que usa ACP por
/// debajo, no lo hace: le entrega la ruta a Windows tal cual.
///
/// Así que el enrutado se hace acá: los guiones salen por `cmd /C`, y los
/// ejecutables de verdad se lanzan directo.
///
/// **Límite conocido:** una ruta con espacios puede confundir a `cmd`, que
/// tiene reglas de comillas propias. Ninguno de los instaladores de estos CLIs
/// pone el shim en una ruta con espacios; si alguna vez pasa, se va a ver como
/// un «no se encontró el programa» y hay que mirar acá.
pub fn launcher(program: &str) -> Option<(PathBuf, Vec<String>)> {
    let path = resolve(program)?;
    #[cfg(windows)]
    {
        let es_guion = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("cmd") || e.eq_ignore_ascii_case("bat"))
            .unwrap_or(false);
        if es_guion {
            let interprete = std::env::var_os("COMSPEC")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("cmd.exe"));
            return Some((
                interprete,
                vec!["/C".to_string(), path.display().to_string()],
            ));
        }
    }
    Some((path, Vec::new()))
}

/// ¿Este binario está en el PATH (con la misma regla que al spawnear)?
#[tauri::command]
pub fn cli_on_path(name: String) -> bool {
    resolve(name.trim()).is_some()
}

/// El núcleo, con el entorno inyectado.
///
/// Separado de [`resolve`] para poder probarlo: la versión pública depende del
/// PATH de la máquina, y un test que dependa de eso pasa acá y falla en CI.
fn resolve_with(
    program: &str,
    dirs: &[PathBuf],
    exts: &[String],
    bare_ok: bool,
    exists: impl Fn(&Path) -> bool,
) -> Option<PathBuf> {
    if program.is_empty() {
        return None;
    }

    // Ya es una ruta: se respeta tal cual, probando las extensiones por si vino
    // sin ella. Una ruta escrita a mano vale aunque no tenga extensión — quien
    // la escribió sabía lo que hacía.
    if program.contains('/') || program.contains('\\') {
        let direct = PathBuf::from(program);
        if direct.extension().is_some() && exists(&direct) {
            return Some(direct);
        }
        if let Some(found) = con_extensiones(&direct, exts, &exists) {
            return Some(found);
        }
        return (bare_ok && exists(&direct)).then_some(direct);
    }

    for dir in dirs {
        let base = dir.join(program);
        // Las extensiones ANTES que el nombre pelado. npm deja TRES archivos
        // juntos —`opencode` (guion de shell), `.cmd` y `.ps1`— y el pelado va
        // primero en orden alfabético: probarlo antes devolvía una ruta que
        // existe, no es ejecutable en Windows, y explotaba al lanzarla.
        if let Some(found) = con_extensiones(&base, exts, &exists) {
            return Some(found);
        }
        if bare_ok && exists(&base) {
            return Some(base);
        }
    }
    None
}

fn con_extensiones(
    base: &Path,
    exts: &[String],
    exists: &impl Fn(&Path) -> bool,
) -> Option<PathBuf> {
    for ext in exts {
        let mut name = OsString::from(base.as_os_str());
        name.push(ext);
        let candidate = PathBuf::from(name);
        if exists(&candidate) {
            return Some(candidate);
        }
    }
    None
}

fn dirs_from_env() -> Vec<PathBuf> {
    std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default()
}

/// Fusión sin duplicados, con el proceso primero: su orden ya resolvía bien y
/// solo se le suman al final las carpetas que el registro trae de nuevas.
/// Windows no distingue mayúsculas en rutas.
#[cfg(windows)]
fn merge_path_dirs(process: Vec<PathBuf>, fresh: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut seen: Vec<String> = process
        .iter()
        .map(|d| d.to_string_lossy().to_lowercase())
        .collect();
    let mut out = process;
    for dir in fresh {
        let key = dir.to_string_lossy().to_lowercase();
        if !key.is_empty() && !seen.contains(&key) {
            seen.push(key);
            out.push(dir);
        }
    }
    out
}

/// El PATH vigente en el registro (usuario + sistema). `RegGetValueW` con
/// `RRF_RT_REG_SZ` ya expande los `REG_EXPAND_SZ` (`%SystemRoot%`…).
#[cfg(windows)]
fn dirs_from_registry() -> Vec<PathBuf> {
    use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    let mut out = Vec::new();
    for (root, subkey) in [
        (HKEY_CURRENT_USER, "Environment"),
        (
            HKEY_LOCAL_MACHINE,
            r"SYSTEM\CurrentControlSet\Control\Session Manager\Environment",
        ),
    ] {
        if let Some(value) = registry_path_value(root, subkey) {
            out.extend(std::env::split_paths(&value));
        }
    }
    out
}

#[cfg(windows)]
fn registry_path_value(
    root: windows_sys::Win32::System::Registry::HKEY,
    subkey: &str,
) -> Option<String> {
    use windows_sys::Win32::System::Registry::{RegGetValueW, RRF_RT_REG_SZ};

    let subkey_w: Vec<u16> = subkey.encode_utf16().chain(std::iter::once(0)).collect();
    let value_w: Vec<u16> = "Path".encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        let mut size: u32 = 0;
        if RegGetValueW(
            root,
            subkey_w.as_ptr(),
            value_w.as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut size,
        ) != 0
            || size == 0
        {
            return None;
        }
        // `size` viene en bytes, con el NUL incluido.
        let mut buf = vec![0u16; size.div_ceil(2) as usize];
        if RegGetValueW(
            root,
            subkey_w.as_ptr(),
            value_w.as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            buf.as_mut_ptr().cast(),
            &mut size,
        ) != 0
        {
            return None;
        }
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        Some(String::from_utf16_lossy(&buf[..len]))
    }
}

/// Las extensiones a probar, en orden de preferencia.
#[cfg(windows)]
fn exts_from_env() -> Vec<String> {
    // Se cruza lo que declara el sistema con lo que `Command` sabe lanzar, y se
    // ordena por LANZABLES y no por PATHEXT: si hay `foo.exe` y `foo.cmd`, el
    // ejecutable real es el que hay que usar, aunque el sistema liste `.cmd`
    // primero.
    let declaradas = std::env::var("PATHEXT").unwrap_or_default().to_lowercase();
    LANZABLES
        .iter()
        .filter(|e| declaradas.is_empty() || declaradas.contains(*e))
        .map(|e| e.to_string())
        .collect()
}

/// En Unix un ejecutable no lleva extensión y `Command` ya resuelve el PATH
/// solo. Esto queda por simetría, para que el adaptador no tenga ramas por
/// sistema operativo.
#[cfg(not(windows))]
fn exts_from_env() -> Vec<String> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// En Windows el nombre pelado NO se ejecuta; en Unix sí. Los tests fijan
    /// las dos reglas por separado en vez de depender de dónde corren.
    const WIN: bool = false;
    const UNIX: bool = true;

    fn entorno(archivos: &[&str]) -> (Vec<PathBuf>, Vec<String>, HashSet<PathBuf>) {
        let dirs = vec![PathBuf::from("/uno"), PathBuf::from("/dos")];
        let exts = [".exe", ".com", ".cmd", ".bat"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let hay = archivos.iter().map(PathBuf::from).collect();
        (dirs, exts, hay)
    }

    /// El caso que rompía: shim de npm sin `.exe` al lado.
    #[test]
    fn encuentra_el_shim_cmd_de_npm() {
        let (dirs, exts, hay) = entorno(&["/dos/opencode.cmd"]);
        let found = resolve_with("opencode", &dirs, &exts, WIN, |p| hay.contains(p));
        assert_eq!(found, Some(PathBuf::from("/dos/opencode.cmd")));
    }

    /// La regresión de verdad: npm deja los TRES juntos y el pelado va primero
    /// en orden alfabético. Elegirlo devolvía una ruta válida que Windows no
    /// puede ejecutar, y el fallo aparecía recién al lanzar el proceso con
    /// «%1 is not a valid Win32 application».
    #[test]
    fn el_guion_sin_extension_no_le_gana_al_cmd() {
        let (dirs, exts, hay) =
            entorno(&["/uno/opencode", "/uno/opencode.cmd", "/uno/opencode.ps1"]);
        let found = resolve_with("opencode", &dirs, &exts, WIN, |p| hay.contains(p));
        assert_eq!(found, Some(PathBuf::from("/uno/opencode.cmd")));
    }

    /// Y en Unix ese mismo archivo pelado es exactamente lo que hay que correr.
    #[test]
    fn en_unix_el_pelado_si_sirve() {
        let (dirs, _, hay) = entorno(&["/uno/opencode"]);
        let found = resolve_with("opencode", &dirs, &[], UNIX, |p| hay.contains(p));
        assert_eq!(found, Some(PathBuf::from("/uno/opencode")));
    }

    /// El caso que ya andaba: un `.exe` de verdad.
    #[test]
    fn encuentra_el_exe_directo() {
        let (dirs, exts, hay) = entorno(&["/uno/claude.exe"]);
        let found = resolve_with("claude", &dirs, &exts, WIN, |p| hay.contains(p));
        assert_eq!(found, Some(PathBuf::from("/uno/claude.exe")));
    }

    /// Con los dos presentes gana el ejecutable real, no el envoltorio.
    #[test]
    fn el_exe_le_gana_al_cmd() {
        let (dirs, exts, hay) = entorno(&["/uno/foo.cmd", "/uno/foo.exe"]);
        let found = resolve_with("foo", &dirs, &exts, WIN, |p| hay.contains(p));
        assert_eq!(found, Some(PathBuf::from("/uno/foo.exe")));
    }

    /// El orden del PATH manda entre directorios.
    #[test]
    fn el_primer_directorio_del_path_gana() {
        let (dirs, exts, hay) = entorno(&["/uno/foo.cmd", "/dos/foo.exe"]);
        let found = resolve_with("foo", &dirs, &exts, WIN, |p| hay.contains(p));
        assert_eq!(
            found,
            Some(PathBuf::from("/uno/foo.cmd")),
            "un PATH temprano gana aunque el de más allá sea mejor extensión"
        );
    }

    /// `.ps1` está en PATHEXT y NO se puede lanzar: devolverlo solo movería el
    /// fallo a más tarde.
    #[test]
    fn no_devuelve_un_ps1() {
        let (dirs, exts, hay) = entorno(&["/uno/opencode.ps1"]);
        assert_eq!(
            resolve_with("opencode", &dirs, &exts, WIN, |p| hay.contains(p)),
            None
        );
    }

    /// Una ruta explícita se respeta sin recorrer el PATH.
    #[test]
    fn una_ruta_explicita_no_se_busca() {
        let (_, exts, hay) = entorno(&["/otro/sitio/opencode.cmd"]);
        let found = resolve_with(
            "/otro/sitio/opencode.cmd",
            &[PathBuf::from("/uno")],
            &exts,
            WIN,
            |p| hay.contains(p),
        );
        assert_eq!(found, Some(PathBuf::from("/otro/sitio/opencode.cmd")));
    }

    /// Y si vino sin extensión, se le prueban igual.
    #[test]
    fn una_ruta_explicita_tambien_prueba_extensiones() {
        let (_, exts, hay) = entorno(&["/otro/opencode.cmd"]);
        let found = resolve_with("/otro/opencode", &[], &exts, WIN, |p| hay.contains(p));
        assert_eq!(found, Some(PathBuf::from("/otro/opencode.cmd")));
    }

    /// La fusión suma solo lo nuevo del registro, al final y sin repetir
    /// (las rutas de Windows no distinguen mayúsculas).
    #[cfg(windows)]
    #[test]
    fn fusiona_proceso_primero_y_sin_duplicados() {
        let merged = merge_path_dirs(
            vec![PathBuf::from(r"C:\uno"), PathBuf::from(r"C:\dos")],
            vec![
                PathBuf::from(r"c:\DOS"),
                PathBuf::from(r"C:\tres"),
                PathBuf::from(""),
            ],
        );
        assert_eq!(
            merged,
            vec![
                PathBuf::from(r"C:\uno"),
                PathBuf::from(r"C:\dos"),
                PathBuf::from(r"C:\tres"),
            ]
        );
    }

    #[test]
    fn lo_que_no_esta_devuelve_nada() {
        let (dirs, exts, hay) = entorno(&["/uno/otra_cosa.exe"]);
        assert_eq!(
            resolve_with("opencode", &dirs, &exts, WIN, |p| hay.contains(p)),
            None
        );
        assert_eq!(
            resolve_with("", &dirs, &exts, WIN, |p| hay.contains(p)),
            None
        );
    }
}
