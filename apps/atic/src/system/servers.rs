//! Servidores locales: lo que se levantó para desarrollar (`next dev`, Vite, `python -m
//! http.server`…) y sigue escuchando un puerto. Uno que lleva rato levantado sin ninguna
//! conexión probablemente se olvidó: Rendimiento lo avisa y deja detenerlo.
//!
//! En Windows se leen las tablas TCP (`GetExtendedTcpTable`: quién escucha qué puerto y
//! cuántas conexiones tiene), la línea de comandos de cada proceso (para saber qué es y de
//! qué proyecto) y cuándo empezó. Solo cuentan los entornos de desarrollo (`RUNTIMES`): un
//! navegador o un servicio de Windows también escuchan puertos y no son del usuario.

/// Desde cuánto sin conexiones se da por olvidado.
pub const IDLE_AFTER: u64 = 60 * 60;

/// Los ejecutables que levantan servidores de desarrollo (sin `.exe`, en minúsculas).
const RUNTIMES: [&str; 9] = ["node", "bun", "deno", "python", "python3", "pythonw", "ruby", "php", "dotnet"];

#[derive(Clone, Debug, PartialEq)]
pub struct Server {
    pub pid: u32,
    /// Los puertos que escucha, de menor a mayor.
    pub ports: Vec<u16>,
    /// «Next.js», «Vite», «node»…
    pub label: String,
    /// La carpeta del proyecto, si se sabe por la línea de comandos.
    pub project: Option<String>,
    /// Hace cuántos segundos empezó.
    pub age: Option<u64>,
    /// Conexiones abiertas a sus puertos ahora.
    pub connections: usize,
}

impl Server {
    /// Lleva más de `IDLE_AFTER` levantado y nadie le habla.
    pub fn idle(&self) -> bool {
        self.connections == 0 && self.age.is_some_and(|age| age >= IDLE_AFTER)
    }

    /// «Next.js · mi-proyecto».
    pub fn title(&self) -> String {
        match &self.project {
            Some(project) => format!("{} · {project}", self.label),
            None => self.label.clone(),
        }
    }
}

/// ¿Es un ejecutable de los que levantan servidores de desarrollo?
fn is_runtime(exe: &str) -> bool {
    RUNTIMES.contains(&exe.trim_end_matches(".exe").to_ascii_lowercase().as_str())
}

/// Qué es, por su línea de comandos; si no se reconoce, el ejecutable.
pub fn label_of(command: &str, exe: &str) -> String {
    let line = command.to_ascii_lowercase().replace('\\', "/");
    let has = |needle: &str| line.contains(needle);
    let known = [
        (has("/next/") || has("next dev") || has("next start") || has("next-server"), "Next.js"),
        (has("/vite/") || has("vite.js") || has(" vite"), "Vite"),
        (has("/astro/") || has("astro dev"), "Astro"),
        (has("/nuxt") || has("nuxi"), "Nuxt"),
        (has("/@remix-run/") || has("remix dev"), "Remix"),
        (has("/@sveltejs/") || has("svelte-kit"), "SvelteKit"),
        (has("/storybook/") || has("storybook dev"), "Storybook"),
        (has("/expo/") || has("expo start"), "Expo"),
        (has("webpack") && has("serve"), "webpack"),
        (has("/@angular/cli/") || has("ng serve"), "Angular"),
        (has("http.server"), "Python http.server"),
        (has("uvicorn"), "Uvicorn"),
        (has("flask"), "Flask"),
        (has("manage.py") && has("runserver"), "Django"),
        (has("rails") && has("server"), "Rails"),
        (has("artisan") && has("serve"), "Laravel"),
        (has("php") && has(" -s "), "PHP"),
    ];
    match known.iter().find(|(hit, _)| *hit) {
        Some((_, name)) => name.to_string(),
        None => exe.trim_end_matches(".exe").to_string(),
    }
}

/// La carpeta del proyecto, por la línea de comandos: la que contiene `node_modules` en la
/// ruta de un script, o la del script mismo (`app.py`, `manage.py`).
pub fn project_of(command: &str) -> Option<String> {
    for token in split_command(command) {
        let path = token.replace('\\', "/");
        if let Some(at) = path.find("/node_modules/") {
            return last_segment(&path[..at]);
        }
    }
    for token in split_command(command).iter().skip(1) {
        let path = token.replace('\\', "/");
        let script = [".py", ".js", ".mjs", ".ts", ".rb", ".php"].iter().any(|ext| path.ends_with(ext));
        if script && path.contains('/') {
            return path.rfind('/').and_then(|at| last_segment(&path[..at]));
        }
    }
    None
}

fn last_segment(path: &str) -> Option<String> {
    path.trim_end_matches('/').rsplit('/').next().filter(|s| !s.is_empty() && !s.ends_with(':')).map(str::to_string)
}

/// Las palabras de una línea de comandos, respetando las comillas dobles.
fn split_command(command: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    for c in command.chars() {
        match c {
            '"' => quoted = !quoted,
            c if c.is_whitespace() && !quoted => {
                if !current.is_empty() {
                    out.push(std::mem::take(&mut current));
                }
            }
            c => current.push(c),
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

/// Un puerto que se escucha y su proceso, más las conexiones abiertas (por puerto local).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Socket {
    pub pid: u32,
    pub port: u16,
    pub listening: bool,
}

/// Junta las tablas TCP en servidores: los procesos de `RUNTIMES` que escuchan, cada uno con
/// sus puertos y cuántas conexiones abiertas tienen. `describe(pid)` da su ejecutable, su
/// línea de comandos y su edad; `None` si no es de los que cuentan.
pub fn gather(sockets: &[Socket], own: u32, mut describe: impl FnMut(u32) -> Option<(String, String, Option<u64>)>) -> Vec<Server> {
    use std::collections::BTreeMap;
    let mut listening: BTreeMap<u32, Vec<u16>> = BTreeMap::new();
    for socket in sockets.iter().filter(|s| s.listening && s.pid != own && s.pid != 0) {
        let ports = listening.entry(socket.pid).or_default();
        if !ports.contains(&socket.port) {
            ports.push(socket.port);
        }
    }
    let mut out = Vec::new();
    for (pid, mut ports) in listening {
        let Some((exe, command, age)) = describe(pid) else {
            continue;
        };
        if !is_runtime(&exe) {
            continue;
        }
        ports.sort_unstable();
        let connections = sockets.iter().filter(|s| !s.listening && s.pid == pid && ports.contains(&s.port)).count();
        out.push(Server { pid, ports, label: label_of(&command, &exe), project: project_of(&command), age, connections });
    }
    // Los olvidados primero; después, los más viejos.
    out.sort_by(|a, b| b.idle().cmp(&a.idle()).then(b.age.cmp(&a.age)));
    out
}

#[cfg(windows)]
pub use win::{list, stop};

#[cfg(not(windows))]
pub fn list() -> Vec<Server> {
    Vec::new()
}

#[cfg(not(windows))]
pub fn stop(_: u32) -> Result<(), String> {
    Err(crate::i18n::t("pill.system.servers.stopFailed").to_string())
}

#[cfg(windows)]
mod win {
    use std::collections::HashMap;
    use std::ffi::c_void;
    use std::sync::Mutex;

    use windows_sys::Win32::Foundation::{CloseHandle, FILETIME, HANDLE};
    use windows_sys::Win32::System::Threading::{GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};

    use super::{gather, Server, Socket};

    // Ya enlazadas por `windows`: se declaran aquí para no sumar funciones al Cargo.toml.
    #[link(name = "iphlpapi")]
    extern "system" {
        fn GetExtendedTcpTable(table: *mut c_void, size: *mut u32, order: i32, family: u32, class: i32, reserved: u32) -> u32;
    }
    #[link(name = "ntdll")]
    extern "system" {
        fn NtQueryInformationProcess(process: HANDLE, class: u32, info: *mut c_void, length: u32, returned: *mut u32) -> i32;
    }

    const AF_INET: u32 = 2;
    const AF_INET6: u32 = 23;
    const TCP_TABLE_OWNER_PID_ALL: i32 = 5;
    const LISTEN: u32 = 2;
    const ESTABLISHED: u32 = 5;
    /// `ProcessCommandLineInformation`.
    const COMMAND_LINE: u32 = 60;

    /// Lo leído de cada proceso: no cambia mientras viva (pid + cuándo empezó).
    static CACHE: Mutex<Option<HashMap<u32, (u64, String)>>> = Mutex::new(None);

    pub fn list() -> Vec<Server> {
        let sockets = [sockets(AF_INET), sockets(AF_INET6)].concat();
        let names: HashMap<u32, String> = crate::agents::process_snapshot().into_iter().map(|(pid, _, name)| (pid, name)).collect();
        let now = filetime_now();
        let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
        let cache = cache.get_or_insert_with(HashMap::new);
        cache.retain(|pid, _| names.contains_key(pid));
        gather(&sockets, std::process::id(), |pid| {
            let exe = names.get(&pid)?.clone();
            if !super::is_runtime(&exe) {
                return Some((exe, String::new(), None));
            }
            let started = started(pid);
            let command = match (cache.get(&pid), started) {
                (Some((at, command)), Some(start)) if *at == start => command.clone(),
                _ => {
                    let command = command_line(pid).unwrap_or_default();
                    if let Some(start) = started {
                        cache.insert(pid, (start, command.clone()));
                    }
                    command
                }
            };
            let age = started.map(|start| now.saturating_sub(start) / 10_000_000);
            Some((exe, command, age))
        })
    }

    /// Detiene el servidor y lo que lanzó (`next dev` corre su servidor en un proceso hijo).
    pub fn stop(pid: u32) -> Result<(), String> {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let status = std::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .creation_flags(CREATE_NO_WINDOW)
            .status()
            .map_err(|e| e.to_string())?;
        if status.success() {
            Ok(())
        } else {
            Err(crate::i18n::t("pill.system.servers.stopFailed").to_string())
        }
    }

    /// Las filas de una tabla TCP (IPv4 o IPv6) que escuchan o están conectadas.
    fn sockets(family: u32) -> Vec<Socket> {
        let mut size = 0u32;
        unsafe { GetExtendedTcpTable(std::ptr::null_mut(), &mut size, 0, family, TCP_TABLE_OWNER_PID_ALL, 0) };
        if size == 0 {
            return Vec::new();
        }
        // Con margen: entre las dos llamadas pueden aparecer conexiones.
        let mut buffer = vec![0u32; (size as usize / 4) + 256];
        size = (buffer.len() * 4) as u32;
        if unsafe { GetExtendedTcpTable(buffer.as_mut_ptr().cast(), &mut size, 0, family, TCP_TABLE_OWNER_PID_ALL, 0) } != 0 {
            return Vec::new();
        }
        let count = buffer[0] as usize;
        // IPv4: estado, dirección local, puerto local, dirección remota, puerto remoto, pid.
        // IPv6: dirección local (4), scope, puerto local, dirección remota (4), scope, puerto
        // remoto, estado, pid.
        let (stride, state_at, port_at, pid_at) = if family == AF_INET { (6, 0, 2, 5) } else { (14, 12, 5, 13) };
        let rows = &buffer[1..];
        (0..count)
            .filter_map(|i| {
                let row = rows.get(i * stride..(i + 1) * stride)?;
                let state = row[state_at];
                (state == LISTEN || state == ESTABLISHED).then(|| Socket {
                    pid: row[pid_at],
                    port: u16::from_be((row[port_at] & 0xffff) as u16),
                    listening: state == LISTEN,
                })
            })
            .collect()
    }

    /// Cuándo empezó el proceso (`FILETIME`, en unidades de 100 ns).
    fn started(pid: u32) -> Option<u64> {
        unsafe {
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if handle.is_null() {
                return None;
            }
            let zero = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
            let (mut created, mut exited, mut kernel, mut user) = (zero, zero, zero, zero);
            let ok = GetProcessTimes(handle, &mut created, &mut exited, &mut kernel, &mut user) != 0;
            CloseHandle(handle);
            ok.then(|| ((created.dwHighDateTime as u64) << 32) | created.dwLowDateTime as u64)
        }
    }

    fn filetime_now() -> u64 {
        // De 1601 (FILETIME) a 1970 (UNIX), en unidades de 100 ns.
        const EPOCH: u64 = 116_444_736_000_000_000;
        let since = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
        EPOCH + since.as_nanos() as u64 / 100
    }

    /// La línea de comandos de un proceso del mismo usuario.
    fn command_line(pid: u32) -> Option<String> {
        unsafe {
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if handle.is_null() {
                return None;
            }
            let mut needed = 0u32;
            NtQueryInformationProcess(handle, COMMAND_LINE, std::ptr::null_mut(), 0, &mut needed);
            let mut text = None;
            if needed > 0 && needed < 1 << 20 {
                // Alineado como `UNICODE_STRING` (largo, máximo y el puntero al texto, que
                // apunta dentro del mismo búfer).
                let mut buffer = vec![0u64; (needed as usize).div_ceil(8)];
                if NtQueryInformationProcess(handle, COMMAND_LINE, buffer.as_mut_ptr().cast(), (buffer.len() * 8) as u32, &mut needed) >= 0 {
                    let base = buffer.as_ptr().cast::<u8>();
                    let length = *base.cast::<u16>() as usize / 2;
                    let chars = *base.add(8).cast::<*const u16>();
                    if !chars.is_null() && length > 0 {
                        text = Some(String::from_utf16_lossy(std::slice::from_raw_parts(chars, length)));
                    }
                }
            }
            CloseHandle(handle);
            text
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reconoce_que_es_y_de_que_proyecto() {
        let next = r#""C:\Program Files\nodejs\node.exe" C:\dev\tienda\node_modules\next\dist\server\lib\start-server.js"#;
        assert_eq!(label_of(next, "node.exe"), "Next.js");
        assert_eq!(project_of(next).as_deref(), Some("tienda"));
        let vite = r#"node "C:\Users\yo\OneDrive\Documentos\panel web\node_modules\vite\bin\vite.js""#;
        assert_eq!(label_of(vite, "node.exe"), "Vite");
        assert_eq!(project_of(vite).as_deref(), Some("panel web"));
        let django = r"python C:\dev\api\manage.py runserver";
        assert_eq!(label_of(django, "python.exe"), "Django");
        assert_eq!(project_of(django).as_deref(), Some("api"));
        assert_eq!(label_of("python -m http.server 8000", "python.exe"), "Python http.server");
        assert_eq!(project_of("python -m http.server 8000"), None);
        // Lo que no se reconoce va con su ejecutable.
        assert_eq!(label_of("node server.js", "node.exe"), "node");
    }

    #[test]
    fn junta_los_puertos_y_las_conexiones_de_cada_proceso() {
        let sockets = [
            Socket { pid: 10, port: 3000, listening: true },
            Socket { pid: 10, port: 3000, listening: true },
            Socket { pid: 10, port: 3001, listening: true },
            // El navegador conectado al 3000 (la conexión del lado del servidor).
            Socket { pid: 10, port: 3000, listening: false },
            Socket { pid: 20, port: 5173, listening: true },
            // Un navegador también escucha: no es un servidor de desarrollo.
            Socket { pid: 30, port: 9222, listening: true },
            // Atic mismo no se ofrece.
            Socket { pid: 99, port: 4000, listening: true },
        ];
        let describe = |pid: u32| match pid {
            10 => Some(("node.exe".into(), r"node C:\dev\tienda\node_modules\next\dist\bin\next dev".into(), Some(5 * 3600))),
            20 => Some(("node.exe".into(), r"node C:\dev\panel\node_modules\vite\bin\vite.js".into(), Some(2 * 3600))),
            30 => Some(("chrome.exe".into(), String::new(), Some(100))),
            _ => Some(("node.exe".into(), String::new(), Some(100))),
        };
        let servers = gather(&sockets, 99, describe);
        assert_eq!(servers.len(), 2);
        // El olvidado (sin conexiones, más de una hora) va primero.
        assert_eq!(servers[0].title(), "Vite · panel");
        assert!(servers[0].idle());
        assert_eq!(servers[1].ports, [3000, 3001]);
        assert_eq!(servers[1].connections, 1);
        assert!(!servers[1].idle());
    }
}
