//! CPU, RAM y lista de apps agrupadas por ejecutable.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use super::{SystemApp, SystemSnapshot};

#[derive(Debug, Clone)]
pub struct RawProc {
    pub stem: String,
    pub name: String,
    pub pid: u32,
    #[allow(dead_code)]
    pub cpu_ticks: u64,
    pub ram_bytes: u64,
    /// Sin ventana ni ícono: `node`, `cargo`, un helper de Chrome. Son los que
    /// de verdad se comen la máquina, y antes no se veían.
    pub background: bool,
    /// ¿Es software del usuario? Nada que viva en `/System`, `/usr` o `/bin`
    /// se ofrece para cerrar: matar un demonio del SO no es una feature.
    pub controllable: bool,
}

/// Qué es un proceso, leído de la ruta de su ejecutable.
///
/// Es puro a propósito: la parte difícil (a qué app pertenece un helper) es
/// manipulación de rutas y se prueba sin tocar el SO.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcId {
    /// Clave de agrupación: el bundle si lo hay, si no el ejecutable.
    pub key: String,
    pub name: String,
    pub background: bool,
    pub controllable: bool,
}

/// Árboles del sistema donde no se toca nada.
///
/// `/usr/local` queda fuera de la lista a propósito: ahí vive software del
/// usuario (un `node`, un `python`), y poder matar un proceso propio que se
/// desbocó es justo lo que hace útil al panel.
#[cfg_attr(windows, allow(dead_code))]
const RUTAS_DEL_SO: [&str; 5] = ["/system/", "/usr/", "/bin/", "/sbin/", "/library/apple/"];

/// Identifica un proceso por su ruta.
///
/// La regla que arregla las cifras de Chrome y VS Code: un helper vive DENTRO
/// del bundle de su app (`/Applications/Google Chrome.app/Contents/Frameworks/
/// …/Google Chrome Helper (Renderer).app/…`), así que la clave es el **`.app`
/// más externo**, no el ejecutable. Sin esto, un Chrome de 4 GB se mostraba
/// con los 300 MB de su proceso principal.
#[cfg_attr(windows, allow(dead_code))]
pub fn identify(path: &str) -> ProcId {
    let bajo = path.to_ascii_lowercase();
    // El `.app` más externo: el primero que aparece recorriendo la ruta.
    if let Some(corte) = bajo.find(".app/") {
        let bundle = &path[..corte];
        if let Some(nombre) = bundle.rsplit('/').next().filter(|n| !n.is_empty()) {
            return ProcId {
                key: nombre.to_ascii_lowercase(),
                name: nombre.to_string(),
                background: false,
                controllable: !en_el_so(&bajo),
            };
        }
    }
    let archivo = path.rsplit('/').next().unwrap_or(path);
    ProcId {
        key: archivo.to_ascii_lowercase(),
        name: archivo.to_string(),
        background: true,
        controllable: !en_el_so(&bajo),
    }
}

#[cfg_attr(windows, allow(dead_code))]
fn en_el_so(bajo: &str) -> bool {
    if bajo.starts_with("/usr/local/") {
        return false;
    }
    RUTAS_DEL_SO.iter().any(|raiz| bajo.starts_with(raiz))
}

#[derive(Clone)]
struct CpuSample {
    idle: u64,
    kernel: u64,
    user: u64,
    procs: HashMap<u32, u64>,
}

static LAST_CPU: Mutex<Option<CpuSample>> = Mutex::new(None);

/// Pulso del equipo: lo barato de leer.
#[derive(Debug, Clone, Copy)]
pub struct Vitals {
    pub cpu: f32,
    pub ram_used: u64,
    pub ram_total: u64,
}

/// Reloj propio del muestreo barato.
///
/// Va aparte de `LAST_CPU` a propósito: si el vigilante y el panel se pisaran
/// el mismo estado, cada uno le robaría al otro la mitad de su ventana y los
/// dos mostrarían cifras nerviosas. Cada cadencia lleva su propia memoria.
static LAST_HOST: Mutex<Option<CpuSample>> = Mutex::new(None);

fn store_host(sample: CpuSample) -> Option<CpuSample> {
    let Ok(mut guard) = LAST_HOST.lock() else {
        return None;
    };
    let prev = guard.clone();
    *guard = Some(sample);
    prev
}

/// Pids del último barrido, por clave de app.
///
/// Un proceso de segundo plano (`node`, `cargo`) no es una `NSRunningApplication`:
/// no hay a quién pedirle que se cierre, solo un pid. Esto guarda esa
/// correspondencia para que "cerrar" y "forzar" también valgan ahí. Se
/// reescribe entero en cada lectura: nunca se actúa sobre algo de hace rato.
static LAST_PIDS: Mutex<Option<HashMap<String, Vec<u32>>>> = Mutex::new(None);

/// Pids vivos de esa clave según el último barrido (solo los accionables).
pub fn pids_for(key: &str) -> Vec<u32> {
    let Ok(guard) = LAST_PIDS.lock() else {
        return Vec::new();
    };
    guard
        .as_ref()
        .and_then(|map| map.get(key))
        .cloned()
        .unwrap_or_default()
}

fn remember_pids(rows: &[RawProc]) {
    let mut map: HashMap<String, Vec<u32>> = HashMap::new();
    for row in rows {
        if !row.controllable {
            continue;
        }
        map.entry(row.stem.clone()).or_default().push(row.pid);
    }
    if let Ok(mut guard) = LAST_PIDS.lock() {
        *guard = Some(map);
    }
}

/// ¿Se puede cerrar o forzar esta app? Nunca Atic ni el chrome del SO.
pub fn can_control_stem(stem: &str) -> bool {
    let s = stem.trim_end_matches(".exe").trim().to_ascii_lowercase();
    if s.is_empty() {
        return false;
    }
    if crate::launcher_recents::skip_process_stem(&s) {
        return false;
    }
    !matches!(
        s.as_str(),
        "finder"
            | "windowserver"
            | "loginwindow"
            | "dock"
            | "controlcenter"
            | "systemuiserver"
            | "coreaudiod"
            | "cfprefsd"
    )
}

/// Agrupa procesos del mismo exe. Omite chrome del SO. CPU en 0–100.
pub fn group_apps(rows: Vec<RawProc>, proc_cpu: &HashMap<u32, f32>) -> Vec<SystemApp> {
    remember_pids(&rows);
    let mut map: HashMap<String, SystemApp> = HashMap::new();
    for row in rows {
        if !can_control_stem(&row.stem) {
            continue;
        }
        let cpu = proc_cpu.get(&row.pid).copied().unwrap_or(0.0);
        let entry = map.entry(row.stem.clone()).or_insert_with(|| SystemApp {
            id: row.stem.clone(),
            name: row.name.clone(),
            // Se completa después, ya con la lista recortada y por presupuesto:
            // ver `app_icons::attach`.
            icon: None,
            pid: row.pid,
            cpu: 0.0,
            ram_bytes: 0,
            // Lo del sistema se mira, no se mata. Lo del usuario sí: cerrar
            // es pedirlo por las buenas (TERM / `terminate`), forzar es KILL.
            can_close: row.controllable,
            can_force: row.controllable,
            // Traer al frente pide una ventana; un demonio no tiene.
            can_focus: !row.background,
            background: row.background,
        });
        entry.cpu += cpu;
        entry.ram_bytes = entry.ram_bytes.saturating_add(row.ram_bytes);
        if row.name.len() > entry.name.len() {
            entry.name = row.name;
        }
        if row.ram_bytes > 0 && (entry.ram_bytes == row.ram_bytes || row.pid < entry.pid) {
            entry.pid = row.pid;
        }
    }
    let mut out: Vec<SystemApp> = map.into_values().collect();
    out.sort_by(|a, b| {
        b.cpu
            .partial_cmp(&a.cpu)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.ram_bytes.cmp(&a.ram_bytes))
    });
    // El tope deja sitio a las dos clases: la vista filtra (apps / segundo
    // plano) y recorta. Truncar en 32 acá dejaba fuera todo el segundo plano.
    out.truncate(64);
    for app in &mut out {
        app.cpu = app.cpu.clamp(0.0, 100.0);
    }
    out
}

/// Lo caro dura un pestañeo.
///
/// Dos vistas abiertas (la isla y el float) pedían el snapshot cada una por su
/// lado: el doble de trabajo y ventanas de muestreo partidas, con cifras
/// saltando. Con esto la segunda lectura del mismo instante es la misma.
const SNAPSHOT_TTL: Duration = Duration::from_millis(800);

static LAST_SNAPSHOT: Mutex<Option<(SystemSnapshot, Instant)>> = Mutex::new(None);

pub fn read() -> Result<SystemSnapshot, String> {
    if let Ok(guard) = LAST_SNAPSHOT.lock() {
        if let Some((snap, at)) = guard.as_ref() {
            if at.elapsed() < SNAPSHOT_TTL {
                return Ok(snap.clone());
            }
        }
    }
    let snap = imp::read()?;
    if let Ok(mut guard) = LAST_SNAPSHOT.lock() {
        *guard = Some((snap.clone(), Instant::now()));
    }
    Ok(snap)
}

/// CPU y memoria del equipo, sin enumerar procesos.
///
/// Lo que mira el vigilante cada pocos segundos. Enumerar los ~500 procesos de
/// la máquina para saber si la CPU está alta sería pagar el diagnóstico
/// completo en cada latido; la lista solo hace falta cuando hay que nombrar al
/// culpable.
pub fn vitals() -> Result<Vitals, String> {
    imp::vitals()
}

/// Porcentaje de memoria usada, o 0 si todavía no se sabe.
pub fn ram_percent(v: &Vitals) -> f32 {
    if v.ram_total == 0 {
        return 0.0;
    }
    ((v.ram_used as f64 / v.ram_total as f64) * 100.0) as f32
}

/// Clave de agrupación de un pid vivo, releída del SO. Ver `imp::key_of_pid`.
#[cfg(target_os = "macos")]
pub fn key_of_pid(pid: u32) -> Option<String> {
    imp::key_of_pid(pid)
}

#[cfg(windows)]
fn filetime_u64(high: u32, low: u32) -> u64 {
    ((high as u64) << 32) | low as u64
}

fn store_sample(sample: CpuSample) -> Option<CpuSample> {
    let Ok(mut guard) = LAST_CPU.lock() else {
        return None;
    };
    let prev = guard.clone();
    *guard = Some(sample);
    prev
}

fn cpu_ratio(prev: &CpuSample, next: &CpuSample) -> (f32, HashMap<u32, f32>) {
    let idle_d = next.idle.saturating_sub(prev.idle) as f64;
    let kernel_d = next.kernel.saturating_sub(prev.kernel) as f64;
    let user_d = next.user.saturating_sub(prev.user) as f64;
    let total = kernel_d + user_d;
    let sys = if total <= 0.0 {
        0.0
    } else {
        ((total - idle_d).max(0.0) / total * 100.0) as f32
    };
    let mut procs = HashMap::new();
    if total > 0.0 {
        for (pid, ticks) in &next.procs {
            let before = prev.procs.get(pid).copied().unwrap_or(0);
            let delta = ticks.saturating_sub(before) as f64;
            if delta > 0.0 {
                procs.insert(*pid, (delta / total * 100.0) as f32);
            }
        }
    }
    (sys.clamp(0.0, 100.0), procs)
}

#[cfg(windows)]
mod imp {
    use super::*;
    use windows_sys::Win32::Foundation::{CloseHandle, FILETIME, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    use windows_sys::Win32::System::ProcessStatus::{
        GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
    };
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    use windows_sys::Win32::System::Threading::{
        GetProcessTimes, GetSystemTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    fn wchar_to_string(buf: &[u16]) -> String {
        let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        String::from_utf16_lossy(&buf[..end])
    }

    fn exe_stem(name: &str) -> String {
        name.trim_end_matches(".exe")
            .trim_end_matches(".EXE")
            .to_ascii_lowercase()
    }

    /// CPU + memoria, sin recorrer el toolhelp. Ver el gemelo de macOS.
    pub fn vitals() -> Result<Vitals, String> {
        let mut idle = FILETIME {
            dwLowDateTime: 0,
            dwHighDateTime: 0,
        };
        let mut kernel = idle;
        let mut user = idle;
        let ok = unsafe { GetSystemTimes(&mut idle, &mut kernel, &mut user) };
        if ok == 0 {
            return Err("no se pudo leer el CPU".into());
        }
        let muestra = CpuSample {
            idle: filetime_u64(idle.dwHighDateTime, idle.dwLowDateTime),
            kernel: filetime_u64(kernel.dwHighDateTime, kernel.dwLowDateTime),
            user: filetime_u64(user.dwHighDateTime, user.dwLowDateTime),
            procs: HashMap::new(),
        };
        let cpu = match store_host(muestra.clone()) {
            Some(prev) => cpu_ratio(&prev, &muestra).0,
            None => 0.0,
        };
        let mut mem = unsafe { std::mem::zeroed::<MEMORYSTATUSEX>() };
        mem.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
        if unsafe { GlobalMemoryStatusEx(&mut mem) } == 0 {
            return Err("no se pudo leer la RAM".into());
        }
        let ram_total = mem.ullTotalPhys;
        Ok(Vitals {
            cpu,
            ram_used: ram_total.saturating_sub(mem.ullAvailPhys),
            ram_total,
        })
    }

    pub fn read() -> Result<SystemSnapshot, String> {
        let mut idle = FILETIME {
            dwLowDateTime: 0,
            dwHighDateTime: 0,
        };
        let mut kernel = idle;
        let mut user = idle;
        let ok = unsafe { GetSystemTimes(&mut idle, &mut kernel, &mut user) };
        if ok == 0 {
            return Err("no se pudo leer el CPU".into());
        }
        let snap = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
        if snap.is_null() || snap == INVALID_HANDLE_VALUE {
            return Err("no se pudo listar procesos".into());
        }
        let mut entry = unsafe { std::mem::zeroed::<PROCESSENTRY32W>() };
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut rows = Vec::new();
        let mut proc_ticks = HashMap::new();
        unsafe {
            if Process32FirstW(snap, &mut entry) != 0 {
                loop {
                    let pid = entry.th32ProcessID;
                    if pid > 0 {
                        let exe = wchar_to_string(&entry.szExeFile);
                        let stem = exe_stem(&exe);
                        let (ram, ticks) = process_stats(pid);
                        if ticks > 0 {
                            proc_ticks.insert(pid, ticks);
                        }
                        if !stem.is_empty() {
                            rows.push(RawProc {
                                stem,
                                name: display_name(&exe),
                                pid,
                                cpu_ticks: ticks,
                                ram_bytes: ram,
                                // Windows ya listaba todos los procesos por
                                // nombre de exe: no hay bundles que agrupar ni
                                // una clase "segundo plano" que separar.
                                background: false,
                                controllable: true,
                            });
                        }
                    }
                    if Process32NextW(snap, &mut entry) == 0 {
                        break;
                    }
                }
            }
            CloseHandle(snap);
        }

        let sample = CpuSample {
            idle: filetime_u64(idle.dwHighDateTime, idle.dwLowDateTime),
            kernel: filetime_u64(kernel.dwHighDateTime, kernel.dwLowDateTime),
            user: filetime_u64(user.dwHighDateTime, user.dwLowDateTime),
            procs: proc_ticks,
        };
        let (cpu, proc_cpu) = match store_sample(sample.clone()) {
            Some(prev) => cpu_ratio(&prev, &sample),
            None => (0.0, HashMap::new()),
        };

        let mut mem = unsafe { std::mem::zeroed::<MEMORYSTATUSEX>() };
        mem.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
        let ram_ok = unsafe { GlobalMemoryStatusEx(&mut mem) };
        if ram_ok == 0 {
            return Err("no se pudo leer la RAM".into());
        }
        let ram_total = mem.ullTotalPhys;
        let ram_used = ram_total.saturating_sub(mem.ullAvailPhys);

        // Los íconos se resuelven recién acá, después del recorte de 64: no
        // tiene sentido abrir ejecutables cuya fila la lista no va a mostrar.
        let mut apps = group_apps(rows, &proc_cpu);
        crate::system_control::app_icons::attach(&mut apps);
        Ok(SystemSnapshot {
            cpu,
            ram_used,
            ram_total,
            apps,
        })
    }

    fn display_name(exe: &str) -> String {
        let stem = exe_stem(exe);
        if stem.is_empty() {
            return exe.to_string();
        }
        let mut chars = stem.chars();
        match chars.next() {
            Some(c) => format!("{}{}", c.to_uppercase(), chars.as_str()),
            None => stem,
        }
    }

    fn process_stats(pid: u32) -> (u64, u64) {
        unsafe {
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if handle.is_null() {
                return (0, 0);
            }
            let mut pmc = std::mem::zeroed::<PROCESS_MEMORY_COUNTERS>();
            pmc.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
            let ram = if GetProcessMemoryInfo(handle, &mut pmc, pmc.cb) != 0 {
                pmc.WorkingSetSize as u64
            } else {
                0
            };
            let mut create = FILETIME {
                dwLowDateTime: 0,
                dwHighDateTime: 0,
            };
            let mut exit = create;
            let mut kernel = create;
            let mut user = create;
            let ticks =
                if GetProcessTimes(handle, &mut create, &mut exit, &mut kernel, &mut user) != 0 {
                    filetime_u64(kernel.dwHighDateTime, kernel.dwLowDateTime)
                        + filetime_u64(user.dwHighDateTime, user.dwLowDateTime)
                } else {
                    0
                };
            CloseHandle(handle);
            (ram, ticks)
        }
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use super::*;
    use objc2::rc::autoreleasepool;
    use objc2::runtime::AnyObject;
    use objc2_foundation::{NSArray, NSString};

    #[link(name = "AppKit", kind = "framework")]
    extern "C" {}

    const PROC_ALL_PIDS: u32 = 1;
    const PROC_PIDPATHINFO_MAXSIZE: usize = 4 * 1024;
    const HOST_CPU_LOAD_INFO: i32 = 3;
    const HOST_CPU_LOAD_INFO_COUNT: u32 = 4;
    const PROC_PIDTASKINFO: i32 = 4;

    #[repr(C)]
    struct HostCpuLoadInfo {
        user: u32,
        system: u32,
        idle: u32,
        nice: u32,
    }

    #[repr(C)]
    struct ProcTaskInfo {
        pti_virtual_size: u64,
        pti_resident_size: u64,
        pti_total_user: u64,
        pti_total_system: u64,
        pti_threads_user: u64,
        pti_threads_system: u64,
        pti_policy: i32,
        pti_faults: i32,
        pti_pageins: i32,
        pti_cow_faults: i32,
        pti_messages_sent: i32,
        pti_messages_received: i32,
        pti_syscalls_mach: i32,
        pti_syscalls_unix: i32,
        pti_csw: i32,
        pti_threadnum: i32,
        pti_numrunning: i32,
        pti_priority: i32,
    }

    #[repr(C)]
    struct VmStatistics64 {
        free_count: u32,
        active_count: u32,
        inactive_count: u32,
        wire_count: u32,
        _zero_fill_count: u64,
        _reactivations: u64,
        _pageins: u64,
        _pageouts: u64,
        _faults: u64,
        _cow_faults: u64,
        _lookups: u64,
        _hits: u64,
        _purges: u64,
        purgeable_count: u32,
        _speculative_count: u32,
        decompressions: u64,
        compressions: u64,
        swapins: u64,
        swapouts: u64,
        compressor_page_count: u32,
        throttled_count: u32,
        external_page_count: u32,
        internal_page_count: u32,
        total_uncompressed_pages_in_compressor: u64,
    }

    const HOST_VM_INFO64: i32 = 4;
    const HOST_VM_INFO64_COUNT: u32 = (std::mem::size_of::<VmStatistics64>() / 4) as u32;

    extern "C" {
        fn mach_host_self() -> u32;
        fn host_page_size(host: u32, page: *mut usize) -> i32;
        fn host_statistics(host: u32, flavor: i32, info: *mut i32, count: *mut u32) -> i32;
        fn host_statistics64(host: u32, flavor: i32, info: *mut i32, count: *mut u32) -> i32;
        fn sysctlbyname(
            name: *const i8,
            oldp: *mut u8,
            oldlenp: *mut usize,
            newp: *mut u8,
            newlen: usize,
        ) -> i32;
        fn proc_pidinfo(pid: i32, flavor: i32, arg: u64, buffer: *mut u8, buffersize: i32) -> i32;
        fn proc_listpids(kind: u32, typeinfo: u32, buffer: *mut u8, buffersize: i32) -> i32;
        fn proc_pidpath(pid: i32, buffer: *mut u8, buffersize: u32) -> i32;
    }

    fn nsstring_to_string(value: *mut AnyObject) -> Option<String> {
        if value.is_null() {
            return None;
        }
        let value: &NSString = unsafe { &*value.cast() };
        Some(value.to_string())
    }

    /// CPU + memoria, sin tocar la lista de procesos.
    pub fn vitals() -> Result<Vitals, String> {
        let muestra = host_sample()?;
        let cpu = match store_host(muestra.clone()) {
            Some(prev) => cpu_ratio(&prev, &muestra).0,
            None => 0.0,
        };
        let (ram_used, ram_total) = memory()?;
        Ok(Vitals {
            cpu,
            ram_used,
            ram_total,
        })
    }

    /// Contadores de CPU del equipo. Sin procesos: eso es lo caro.
    fn host_sample() -> Result<CpuSample, String> {
        let host = unsafe { mach_host_self() };
        let mut cpu_info = HostCpuLoadInfo {
            user: 0,
            system: 0,
            idle: 0,
            nice: 0,
        };
        let mut count = HOST_CPU_LOAD_INFO_COUNT;
        let ok = unsafe {
            host_statistics(
                host,
                HOST_CPU_LOAD_INFO,
                &mut cpu_info as *mut HostCpuLoadInfo as *mut i32,
                &mut count,
            )
        };
        if ok != 0 {
            return Err("no se pudo leer el CPU".into());
        }
        Ok(CpuSample {
            idle: cpu_info.idle as u64,
            kernel: cpu_info.system as u64,
            user: (cpu_info.user as u64).saturating_add(cpu_info.nice as u64),
            procs: HashMap::new(),
        })
    }

    pub fn read() -> Result<SystemSnapshot, String> {
        let host = unsafe { mach_host_self() };
        let mut cpu_info = HostCpuLoadInfo {
            user: 0,
            system: 0,
            idle: 0,
            nice: 0,
        };
        let mut count = HOST_CPU_LOAD_INFO_COUNT;
        let cpu_ok = unsafe {
            host_statistics(
                host,
                HOST_CPU_LOAD_INFO,
                &mut cpu_info as *mut HostCpuLoadInfo as *mut i32,
                &mut count,
            )
        };
        if cpu_ok != 0 {
            return Err("no se pudo leer el CPU".into());
        }

        let (rows, proc_ticks) = autoreleasepool(|_| live_procs());

        let sample = CpuSample {
            idle: cpu_info.idle as u64,
            kernel: cpu_info.system as u64,
            user: (cpu_info.user as u64).saturating_add(cpu_info.nice as u64),
            procs: proc_ticks,
        };
        let (cpu, proc_cpu) = match store_sample(sample.clone()) {
            Some(prev) => cpu_ratio(&prev, &sample),
            None => (0.0, HashMap::new()),
        };

        let (ram_used, ram_total) = memory()?;

        Ok(SystemSnapshot {
            cpu,
            ram_used,
            ram_total,
            apps: group_apps(rows, &proc_cpu),
        })
    }

    fn memory() -> Result<(u64, u64), String> {
        let mut total: u64 = 0;
        let mut len = std::mem::size_of::<u64>();
        let name = b"hw.memsize\0";
        let ok = unsafe {
            sysctlbyname(
                name.as_ptr() as *const i8,
                &mut total as *mut u64 as *mut u8,
                &mut len,
                std::ptr::null_mut(),
                0,
            )
        };
        if ok != 0 || total == 0 {
            return Err("no se pudo leer la RAM".into());
        }
        let host = unsafe { mach_host_self() };
        let mut page: usize = 0;
        unsafe { host_page_size(host, &mut page) };
        if page == 0 {
            page = 16384;
        }
        let mut vm = unsafe { std::mem::zeroed::<VmStatistics64>() };
        let mut count = HOST_VM_INFO64_COUNT;
        let vm_ok = unsafe {
            host_statistics64(
                host,
                HOST_VM_INFO64,
                &mut vm as *mut VmStatistics64 as *mut i32,
                &mut count,
            )
        };
        if vm_ok != 0 {
            return Ok((0, total));
        }
        let used_pages = (vm.active_count as u64)
            .saturating_add(vm.wire_count as u64)
            .saturating_add(vm.compressor_page_count as u64);
        let used = used_pages.saturating_mul(page as u64).min(total);
        Ok((used, total))
    }

    /// Todos los procesos, no solo las apps con ícono.
    ///
    /// Antes esto recorría `runningApplications` con `activationPolicy ==
    /// regular`, y eso deja fuera **justo lo que interesa**: `node`, `cargo`,
    /// `rust-analyzer`, Docker, y los helpers donde Chrome y los Electron
    /// guardan casi toda su memoria. Un Chrome de 4 GB se veía con los 300 MB
    /// de su proceso principal.
    ///
    /// Ahora se listan todos los pids y cada uno se identifica por su ruta
    /// ([`identify`]): los helpers caen bajo el `.app` que los contiene —así
    /// las cifras de Chrome son las de Chrome— y lo que no vive en un bundle
    /// queda marcado como segundo plano. `NSWorkspace` se sigue usando, pero
    /// solo para los nombres bonitos.
    fn live_procs() -> (Vec<RawProc>, HashMap<u32, u64>) {
        let nombres = autoreleasepool(|_| app_names());
        let self_pid = std::process::id();
        let mut rows = Vec::new();
        let mut ticks = HashMap::new();
        for pid in all_pids() {
            if pid == 0 || pid == self_pid {
                continue;
            }
            // Sin ruta no hay nada que decir de él (`kernel_task` y compañía).
            let Some(path) = pid_path(pid as i32) else {
                continue;
            };
            let (ram, cpu_ticks) = task_info(pid as i32);
            if ram == 0 && cpu_ticks == 0 {
                continue;
            }
            let mut id = identify(&path);
            if let Some(nombre) = nombres.get(&pid) {
                id.name = nombre.clone();
            }
            if id.key.is_empty() {
                continue;
            }
            if cpu_ticks > 0 {
                ticks.insert(pid, cpu_ticks);
            }
            rows.push(RawProc {
                stem: id.key,
                name: id.name,
                pid,
                cpu_ticks,
                ram_bytes: ram,
                background: id.background,
                controllable: id.controllable,
            });
        }
        (rows, ticks)
    }

    /// Clave de agrupación de un pid vivo, releída del SO.
    ///
    /// Se usa antes de matar por pid: el barrido tiene hasta un segundo y los
    /// pids se reciclan. Si la ruta ya no dice lo mismo, no se toca.
    pub fn key_of_pid(pid: u32) -> Option<String> {
        pid_path(pid as i32).map(|path| identify(&path).key)
    }

    /// Nombre localizado por pid, para no mostrar el del ejecutable.
    fn app_names() -> HashMap<u32, String> {
        let mut out = HashMap::new();
        unsafe {
            let workspace: *mut AnyObject =
                objc2::msg_send![objc2::class!(NSWorkspace), sharedWorkspace];
            if workspace.is_null() {
                return out;
            }
            let apps: *mut AnyObject = objc2::msg_send![workspace, runningApplications];
            if apps.is_null() {
                return out;
            }
            let apps: &NSArray<AnyObject> = &*apps.cast();
            for app in apps.iter() {
                let pid: i32 = objc2::msg_send![&*app, processIdentifier];
                if pid <= 0 {
                    continue;
                }
                if let Some(name) = nsstring_to_string(objc2::msg_send![&*app, localizedName]) {
                    out.insert(pid as u32, name);
                }
            }
        }
        out
    }

    fn all_pids() -> Vec<u32> {
        let falta = unsafe { proc_listpids(PROC_ALL_PIDS, 0, std::ptr::null_mut(), 0) };
        if falta <= 0 {
            return Vec::new();
        }
        // Holgura: entre medir y leer pueden nacer procesos.
        let cupo = (falta as usize / std::mem::size_of::<i32>()) + 64;
        let mut buf = vec![0i32; cupo];
        let bytes = unsafe {
            proc_listpids(
                PROC_ALL_PIDS,
                0,
                buf.as_mut_ptr() as *mut u8,
                (cupo * std::mem::size_of::<i32>()) as i32,
            )
        };
        if bytes <= 0 {
            return Vec::new();
        }
        let n = bytes as usize / std::mem::size_of::<i32>();
        buf.into_iter()
            .take(n)
            .filter(|pid| *pid > 0)
            .map(|pid| pid as u32)
            .collect()
    }

    fn pid_path(pid: i32) -> Option<String> {
        let mut buf = vec![0u8; PROC_PIDPATHINFO_MAXSIZE];
        let n = unsafe { proc_pidpath(pid, buf.as_mut_ptr(), buf.len() as u32) };
        if n <= 0 {
            return None;
        }
        buf.truncate(n as usize);
        String::from_utf8(buf).ok()
    }

    fn task_info(pid: i32) -> (u64, u64) {
        let mut info = unsafe { std::mem::zeroed::<ProcTaskInfo>() };
        let size = std::mem::size_of::<ProcTaskInfo>() as i32;
        let n = unsafe {
            proc_pidinfo(
                pid,
                PROC_PIDTASKINFO,
                0,
                &mut info as *mut ProcTaskInfo as *mut u8,
                size,
            )
        };
        if n < size {
            return (0, 0);
        }
        let ticks = info.pti_total_user.saturating_add(info.pti_total_system);
        (info.pti_resident_size, ticks)
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
mod imp {
    use super::*;
    pub fn read() -> Result<SystemSnapshot, String> {
        Err("no soportado en esta plataforma todavía".into())
    }
    pub fn vitals() -> Result<Vitals, String> {
        Err("no soportado en esta plataforma todavía".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proc(stem: &str, name: &str, pid: u32, ram: u64) -> RawProc {
        RawProc {
            stem: stem.into(),
            name: name.into(),
            pid,
            cpu_ticks: 0,
            ram_bytes: ram,
            background: false,
            controllable: true,
        }
    }

    #[test]
    fn agrupa_procesos_del_mismo_stem() {
        let rows = vec![
            proc("chrome", "Chrome", 10, 100),
            proc("chrome", "Grok - Google Chrome", 11, 50),
            proc("code", "Code", 20, 80),
        ];
        let mut cpu = HashMap::new();
        cpu.insert(10, 4.0);
        cpu.insert(11, 2.0);
        cpu.insert(20, 1.0);
        let apps = group_apps(rows, &cpu);
        let chrome = apps.iter().find(|a| a.id == "chrome").unwrap();
        assert_eq!(chrome.cpu, 6.0);
        assert_eq!(chrome.ram_bytes, 150);
        assert_eq!(chrome.name, "Grok - Google Chrome");
        assert!(chrome.can_force);
    }

    #[test]
    fn omite_chrome_del_so_y_atic() {
        let rows = vec![
            proc("explorer", "Explorador", 4, 10),
            proc("atic", "Atic", 9, 20),
            proc("dwm", "DWM", 5, 5),
            proc("notes", "Notes", 8, 30),
        ];
        let apps = group_apps(rows, &HashMap::new());
        let ids: Vec<&str> = apps.iter().map(|a| a.id.as_str()).collect();
        assert_eq!(ids, vec!["notes"]);
        assert!(apps[0].can_close);
        assert!(apps[0].can_force);
    }

    #[test]
    fn un_helper_cuenta_como_su_app() {
        // Lo que arregla las cifras de Chrome: el helper pertenece al bundle
        // que lo contiene, no a sí mismo.
        let helper = identify(
            "/Applications/Google Chrome.app/Contents/Frameworks/Google Chrome Framework.framework/Versions/1/Helpers/Google Chrome Helper (Renderer).app/Contents/MacOS/Google Chrome Helper (Renderer)",
        );
        assert_eq!(helper.key, "google chrome");
        assert!(!helper.background);
        assert!(helper.controllable);

        let app = identify("/Applications/Safari.app/Contents/MacOS/Safari");
        assert_eq!(app.key, "safari");
        assert_eq!(app.name, "Safari");
        assert!(!app.background);
    }

    #[test]
    fn lo_que_no_vive_en_un_bundle_es_segundo_plano() {
        let node = identify("/opt/homebrew/bin/node");
        assert_eq!(node.key, "node");
        assert!(node.background, "node no tiene ventana ni ícono");
        assert!(node.controllable, "es software del usuario: se puede matar");

        // `/usr/local` es del usuario aunque cuelgue de `/usr`.
        assert!(identify("/usr/local/bin/python3").controllable);
    }

    #[test]
    fn el_sistema_no_se_toca() {
        assert!(!identify("/usr/libexec/secinitd").controllable);
        assert!(
            !identify("/System/Library/CoreServices/Dock.app/Contents/MacOS/Dock").controllable
        );
        assert!(!identify("/sbin/launchd").controllable);
    }

    #[test]
    fn no_se_puede_forzar_finder() {
        assert!(!can_control_stem("Finder"));
        assert!(!can_control_stem("atic-desktop"));
        assert!(can_control_stem("Safari"));
    }
}
