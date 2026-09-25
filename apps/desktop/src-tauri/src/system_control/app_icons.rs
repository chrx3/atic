//! Ruta del ejecutable de un pid, nombre visible e ícono de la app.
//!
//! Windows no entrega el ícono de un proceso: hay que resolver primero su
//! `.exe` y después pedirle la imagen al shell. Las dos cosas abren archivos,
//! así que el panel de recursos guarda el resultado —y también el fallo— entre
//! vueltas. Antes estas funciones vivían dentro de `audio`; ahora son la única
//! copia que usan las dos pestañas.

use std::collections::HashMap;
#[cfg(windows)]
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use atic_core::MutexExt;

use super::SystemApp;

/// Cuántos íconos NUEVOS se resuelven por vuelta del panel.
///
/// El panel pide el snapshot cada 1,5 s y la lista llega a 64 apps. Resolver
/// las 64 de una vez sería abrir 64 ejecutables y pedirle 64 veces la imagen
/// al shell en el mismo tick: el primer pintado se sentiría pesado justo
/// cuando el panel tiene que abrir. Con 12 por vuelta, la parte de arriba de la
/// lista —la que ordenan CPU y RAM, que es la que uno mira— se llena en el
/// primer tick y el resto en los siguientes, sin que ninguna vuelta pase de
/// una docena de aperturas. Lo ya resuelto queda cacheado y no se repite.
const PRESUPUESTO_POR_VUELTA: usize = 12;

/// Tope de la caché de íconos, en claves.
///
/// La lista de apps se recorta en 64, así que 256 entradas sobran para todas
/// las apps vivas y dejan sitio a las que vayan apareciendo; cuando se llena
/// se tira entera, igual que la caché de nombres. Vaciar una caché de íconos
/// no desborda: gracias al presupuesto, volver a llenarla cuesta unas pocas
/// vueltas y nunca una sola.
const TOPE_CACHE: usize = 256;

/// `stem` → data URL del ícono, incluido el fallo (`None`).
///
/// La clave es el mismo `stem` que el panel usa como `id` (el nombre del
/// `.exe`), que es estable entre vueltas. El pid NO sirve: el grupo cambia de
/// pid cuando aparece otro proceso del mismo ejecutable con más memoria.
///
/// Memorizar el fallo es la mitad del asunto: de las 64 apps, varias son
/// procesos protegidos cuyo ícono nunca se va a poder sacar, y sin el `None`
/// cacheado cada vuelta de 1,5 s volvería a intentarlo para siempre. La caché
/// de `launcher_icons` no alcanza como protección porque a propósito no
/// guarda los fallos.
fn cache() -> &'static Mutex<HashMap<String, Option<String>>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Option<String>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Completa el `icon` de cada app que ya se pueda resolver.
///
/// Solo se resuelven las claves que faltan y, de esas, a lo sumo
/// [`PRESUPUESTO_POR_VUELTA`]: lo que queda afuera conserva el hueco vacío y se
/// reintenta en la vuelta siguiente. Lo postergado no se cachea, porque "todavía
/// no" no es lo mismo que "no se pudo".
pub fn attach(apps: &mut [SystemApp]) {
    attach_with(apps, |app| resolve(app.pid));
}

/// El reparto de [`attach`], con el resolutor inyectable para poder probar el
/// presupuesto y la memoria de fallos sin tocar el shell.
fn attach_with(apps: &mut [SystemApp], mut resolver: impl FnMut(&SystemApp) -> Option<String>) {
    let mut nuevas = 0usize;
    for app in apps.iter_mut() {
        // El guard se suelta en esta misma sentencia: adentro del `if` se
        // vuelve a bloquear el mismo mutex y no puede quedar vivo.
        let guardado = cache().lock_or_recover().get(&app.id).cloned();
        if let Some(hit) = guardado {
            app.icon = hit;
            continue;
        }
        if nuevas >= PRESUPUESTO_POR_VUELTA {
            continue;
        }
        nuevas += 1;
        let icono = resolver(app);
        let mut guard = cache().lock_or_recover();
        if guard.len() >= TOPE_CACHE {
            guard.clear();
        }
        guard.insert(app.id.clone(), icono.clone());
        app.icon = icono;
    }
}

/// Ícono de la app: ruta real del proceso → imagen que da el shell.
///
/// La ruta se le pide al pid que el panel eligió para el grupo (el de más
/// memoria). Si ese proceso no deja leer su ruta —normal entre los protegidos—
/// queda `None` y se recuerda.
#[cfg(windows)]
fn resolve(pid: u32) -> Option<String> {
    let path = process_path(pid)?;
    crate::launcher_icons::icon_data_url(&path)
}

/// En macOS el ícono es el del `.app` más externo que contiene al proceso,
/// el mismo que usa el panel para agrupar: así un helper de Chrome muestra el
/// de Chrome. Lo que no vive en un bundle (`node`, `cargo`) queda sin ícono y
/// el panel pinta su inicial.
#[cfg(target_os = "macos")]
fn resolve(pid: u32) -> Option<String> {
    let path = super::snapshot::pid_path(pid)?;
    let bundle = super::snapshot::outer_bundle(&path)?;
    crate::launcher_icons::icon_data_url(std::path::Path::new(bundle))
}

#[cfg(windows)]
/// Ruta del ejecutable del proceso.
///
/// Mismo patrón que el resto de la app (`meeting_detection`, `apps`,
/// `clipboard_history`): `OpenProcess` + `QueryFullProcessImageNameW` con
/// permisos limitados, que alcanzan para leer la ruta de otro proceso.
pub fn process_path(pid: u32) -> Option<PathBuf> {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return None;
        }
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(handle, 0, buf.as_mut_ptr(), &mut len);
        CloseHandle(handle);
        if ok == 0 || len == 0 {
            return None;
        }
        Some(PathBuf::from(String::from_utf16_lossy(
            &buf[..len as usize],
        )))
    }
}

/// `FileDescription` por ruta ya resuelta.
///
/// El panel repinta las pestañas cada pocos segundos y leer el VERSIONINFO
/// abre el .exe en cada vuelta; el recurso de un archivo no cambia mientras
/// la app corre, así que se guarda. Acotada: si crece, se tira entera.
///
/// Se cachea también el fallo (`None`): repetir la apertura no lo arregla.
#[cfg(windows)]
fn name_cache() -> &'static Mutex<HashMap<String, Option<String>>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Option<String>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

#[cfg(windows)]
/// Nombre visible del ejecutable: `FileDescription` del VERSIONINFO —el que
/// muestra el Administrador de tareas— y, si el archivo no trae recurso, el
/// nombre del ejecutable sin extensión.
pub fn app_name(path: &Path) -> Option<String> {
    let key = path.to_string_lossy().to_lowercase();
    // El guard no puede seguir vivo mientras la rama de fallo vuelve a
    // bloquear: si el temporal del `match` lo retuviera, el primer cache
    // miss colgaría el hilo para siempre.
    let cached = name_cache().lock_or_recover().get(&key).cloned();
    let description = match cached {
        Some(hit) => hit,
        None => {
            let hit = file_description(path);
            let mut cache = name_cache().lock_or_recover();
            if cache.len() >= 128 {
                cache.clear();
            }
            cache.insert(key, hit.clone());
            hit
        }
    };
    description.or_else(|| {
        path.file_stem()
            .and_then(|s| s.to_str())
            .map(str::to_string)
    })
}

#[cfg(windows)]
/// `FileDescription` del VERSIONINFO del ejecutable.
///
/// `GetFileVersionInfoW` lee solo el recurso de versión, no el archivo
/// entero. La tabla de traducciones dice en qué idioma está el recurso, así
/// que se prueban sus pares idioma/página hasta dar con uno con texto.
fn file_description(path: &Path) -> Option<String> {
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW,
    };

    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    unsafe {
        let size = GetFileVersionInfoSizeW(wide.as_ptr(), std::ptr::null_mut());
        if size == 0 {
            return None;
        }
        let mut block = vec![0u8; size as usize];
        if GetFileVersionInfoW(wide.as_ptr(), 0, size, block.as_mut_ptr().cast()) == 0 {
            return None;
        }
        let sub: Vec<u16> = "\\VarFileInfo\\Translation\0".encode_utf16().collect();
        let mut translations: *mut c_void = std::ptr::null_mut();
        let mut count = 0u32;
        if VerQueryValueW(
            block.as_ptr().cast(),
            sub.as_ptr(),
            &mut translations,
            &mut count,
        ) == 0
            || count < 4
        {
            return None;
        }
        let pairs = std::slice::from_raw_parts(translations as *const u16, (count / 2) as usize);
        for pair in pairs.chunks_exact(2) {
            let query: Vec<u16> = format!(
                "\\StringFileInfo\\{:04x}{:04x}\\FileDescription\0",
                pair[0], pair[1]
            )
            .encode_utf16()
            .collect();
            let mut value: *mut c_void = std::ptr::null_mut();
            let mut len = 0u32;
            if VerQueryValueW(block.as_ptr().cast(), query.as_ptr(), &mut value, &mut len) == 0
                || len == 0
            {
                continue;
            }
            let text = String::from_utf16_lossy(std::slice::from_raw_parts(
                value as *const u16,
                len as usize,
            ));
            let text = text.trim_end_matches('\0').trim();
            if !text.is_empty() {
                return Some(text.to_string());
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(id: &str) -> SystemApp {
        SystemApp {
            id: id.into(),
            name: id.into(),
            icon: None,
            pid: 1,
            cpu: 0.0,
            ram_bytes: 0,
            can_close: true,
            can_force: true,
            can_focus: true,
            background: false,
        }
    }

    #[cfg(windows)]
    /// El nombre visible de las apps sale del VERSIONINFO del .exe, porque
    /// `GetDisplayName()` viene vacío. `notepad.exe` es un binario que
    /// siempre trae el recurso en Windows.
    #[test]
    fn reads_file_description_from_version_info() {
        let notepad = Path::new("C:\\Windows\\System32\\notepad.exe");
        let description = file_description(notepad).unwrap_or_default();
        assert!(
            !description.is_empty(),
            "sin FileDescription en {notepad:?}"
        );
    }

    #[cfg(windows)]
    /// Sin recurso de versión (o sin archivo) queda el nombre del ejecutable
    /// sin extensión, que es el último escalón antes del "App {pid}".
    #[test]
    fn falls_back_to_executable_stem() {
        let missing = Path::new("C:\\no-existe\\alguna-app.exe");
        assert_eq!(app_name(missing).as_deref(), Some("alguna-app"));
    }

    /// El presupuesto reparte la extracción y el fallo se recuerda.
    ///
    /// Veinte apps nuevas: la primera vuelta resuelve solo el presupuesto, la
    /// segunda termina el resto y ninguna vuelta posterior vuelve a tocar el
    /// shell. Es lo que evita que un proceso protegido (ícono imposible) se
    /// reintente en cada ciclo de 1,5 s para siempre.
    #[test]
    fn el_presupuesto_reparte_y_el_fallo_se_recuerda() {
        let ids: Vec<String> = (0..20).map(|i| format!("zz-prueba-icono-{i}")).collect();
        let mut apps: Vec<SystemApp> = ids.iter().map(|id| app(id)).collect();
        let intentos = std::cell::Cell::new(0usize);
        let mut resolver = |_: &SystemApp| {
            intentos.set(intentos.get() + 1);
            None
        };

        attach_with(&mut apps, &mut resolver);
        assert_eq!(intentos.get(), PRESUPUESTO_POR_VUELTA);
        assert!(apps.iter().all(|a| a.icon.is_none()));

        attach_with(&mut apps, &mut resolver);
        assert_eq!(
            intentos.get(),
            20,
            "la segunda vuelta termina las que faltaban"
        );

        attach_with(&mut apps, &mut resolver);
        assert_eq!(intentos.get(), 20, "los fallos ya no se reintentan");
    }
}
