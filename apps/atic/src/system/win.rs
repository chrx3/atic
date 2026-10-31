//! Windows: Rendimiento (CPU, RAM, GPU, red, discos, batería) y lo que se
//! cambia de las pantallas (resolución, principal, duplicar). Lo llama
//! `os::imp` desde su hilo.

use super::os::{Disk, Mode, Power, Vitals};
use std::time::{Duration, Instant};
use windows::core::{w, BSTR, PCWSTR};
use windows::Win32::Devices::Display::{
    GetDisplayConfigBufferSizes, QueryDisplayConfig, SetDisplayConfig, DISPLAYCONFIG_MODE_INFO,
    DISPLAYCONFIG_PATH_INFO, DISPLAYCONFIG_TOPOLOGY_CLONE, DISPLAYCONFIG_TOPOLOGY_ID,
    QDC_DATABASE_CURRENT, QDC_ONLY_ACTIVE_PATHS, SDC_APPLY, SDC_TOPOLOGY_CLONE,
    SDC_TOPOLOGY_EXTEND,
};
use windows::Win32::Foundation::{FILETIME, POINTL};
use windows::Win32::Graphics::Gdi::{
    ChangeDisplaySettingsExW, EnumDisplayDevicesW, EnumDisplaySettingsExW, CDS_NORESET,
    CDS_SET_PRIMARY, CDS_TYPE, CDS_UPDATEREGISTRY, DEVMODEW, DEVMODEW_0, DEVMODE_FIELD_FLAGS,
    DISPLAY_DEVICEW, DISPLAY_DEVICE_ATTACHED_TO_DESKTOP, DISPLAY_DEVICE_MIRRORING_DRIVER,
    DISPLAY_DEVICE_PRIMARY_DEVICE, DISP_CHANGE, DISP_CHANGE_RESTART, DISP_CHANGE_SUCCESSFUL,
    DM_BITSPERPEL, DM_DISPLAYFREQUENCY, DM_PELSHEIGHT, DM_PELSWIDTH, DM_POSITION,
    ENUM_CURRENT_SETTINGS, ENUM_DISPLAY_SETTINGS_FLAGS, ENUM_DISPLAY_SETTINGS_MODE,
};
use windows::Win32::NetworkManagement::IpHelper::{FreeMibTable, GetIfTable2, MIB_IF_ROW2, MIB_IF_TABLE2};
use windows::Win32::NetworkManagement::Ndis::IfOperStatusUp;
use windows::Win32::Storage::FileSystem::{
    GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDrives, GetVolumeInformationW,
};
use windows::Win32::System::Diagnostics::Debug::{
    SetThreadErrorMode, SEM_FAILCRITICALERRORS, THREAD_ERROR_MODE,
};
use windows::Win32::System::Performance::{
    PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterArrayW,
    PdhOpenQueryW, PDH_CSTATUS_NEW_DATA, PDH_CSTATUS_VALID_DATA, PDH_FMT_COUNTERVALUE_ITEM_W,
    PDH_FMT_DOUBLE, PDH_HCOUNTER, PDH_HQUERY, PDH_MORE_DATA,
};
use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
use windows::Win32::System::SystemInformation::{GetTickCount64, GlobalMemoryStatusEx, MEMORYSTATUSEX};
use windows::Win32::System::Threading::GetSystemTimes;
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::System::Wmi::{
    IWbemClassObject, IWbemServices, WBEM_FLAG_FORWARD_ONLY, WBEM_FLAG_RETURN_IMMEDIATELY,
    WBEM_GENERIC_FLAG_TYPE, WBEM_INFINITE,
};

// `GetDriveTypeW` (están en `WindowsProgramming`, que no se compila).
const DRIVE_REMOVABLE: u32 = 2;
const DRIVE_FIXED: u32 = 3;
const IF_TYPE_SOFTWARE_LOOPBACK: u32 = 24;
// `InterfaceAndOperStatusFlags`: HardwareInterface es el bit 0 y
// FilterInterface el 1 (los filtros de WFP repiten los bytes del adaptador).
const IF_HARDWARE: u8 = 1;
const IF_FILTER: u8 = 2;
/// La capacidad de fábrica, la de hoy y los ciclos casi no cambian.
const BATTERY_EVERY: Duration = Duration::from_secs(60);

/// Lo que hace falta guardar entre lecturas: los porcentajes y las tasas
/// salen de dos muestras.
#[derive(Default)]
pub struct Sampler {
    /// (idle, total) de `GetSystemTimes`.
    cpu: Option<(u64, u64)>,
    net: Option<(u64, u64, Instant)>,
    gpu: Option<Gpu>,
    /// PDH no anda (sin contadores de GPU): no se vuelve a intentar.
    gpu_off: bool,
    /// (cuándo, diseño, carga completa, ciclos).
    battery: Option<(Instant, Option<u32>, Option<u32>, Option<u32>)>,
}

/// La consulta PDH del uso 3D de todos los motores de la GPU.
struct Gpu {
    query: PDH_HQUERY,
    counter: PDH_HCOUNTER,
}

impl Drop for Gpu {
    fn drop(&mut self) {
        // SAFETY: la consulta es nuestra y se cierra una vez.
        unsafe {
            let _ = PdhCloseQuery(self.query);
        }
    }
}

pub(crate) fn vitals(s: &mut Sampler, wmi: Option<&IWbemServices>) -> Vitals {
    let (ram_used, ram_total) = memory();
    let (down, up) = match net_totals() {
        Some((down, up)) => super::os::net_rate(&mut s.net, down, up),
        None => (0.0, 0.0),
    };
    Vitals {
        cpu: cpu(s),
        cpu_temp: None,
        gpu: gpu(s),
        gpu_temp: None,
        ram_used,
        ram_total,
        net_down: down,
        net_up: up,
        disks: disks(),
        thermal: None,
        power: power(s, wmi),
        // SAFETY: sin argumentos.
        uptime: unsafe { GetTickCount64() } / 1000,
    }
}

// --- Rendimiento -------------------------------------------------------------

fn filetime(t: FILETIME) -> u64 {
    (t.dwHighDateTime as u64) << 32 | t.dwLowDateTime as u64
}

fn wide(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

/// UTF-16 terminado en 0, para pasarlo como `PCWSTR`.
fn wide_z(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

/// CPU de todo el equipo entre esta muestra y la anterior (la primera da 0).
fn cpu(s: &mut Sampler) -> f32 {
    let (mut idle, mut kernel, mut user) =
        (FILETIME::default(), FILETIME::default(), FILETIME::default());
    // SAFETY: los tres punteros son locales.
    if unsafe { GetSystemTimes(Some(&mut idle), Some(&mut kernel), Some(&mut user)) }.is_err() {
        return 0.0;
    }
    // El tiempo de kernel ya incluye el ocioso.
    let idle = filetime(idle);
    let total = filetime(kernel) + filetime(user);
    let usage = match s.cpu {
        Some((idle0, total0)) if total > total0 && idle >= idle0 => {
            let busy = (total - total0).saturating_sub(idle - idle0);
            busy as f32 / (total - total0) as f32 * 100.0
        }
        _ => 0.0,
    };
    s.cpu = Some((idle, total));
    usage.clamp(0.0, 100.0)
}

fn memory() -> (u64, u64) {
    let mut status = MEMORYSTATUSEX {
        dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
    // SAFETY: `dwLength` dice el tamaño de la estructura.
    if unsafe { GlobalMemoryStatusEx(&mut status) }.is_err() {
        return (0, 0);
    }
    (status.ullTotalPhys.saturating_sub(status.ullAvailPhys), status.ullTotalPhys)
}

impl Gpu {
    fn open() -> Option<Gpu> {
        // SAFETY: los handles son locales y se cierran si algo falla.
        unsafe {
            let mut query = PDH_HQUERY::default();
            if PdhOpenQueryW(PCWSTR::null(), 0, &mut query) != 0 {
                return None;
            }
            let mut counter = PDH_HCOUNTER::default();
            let path = w!("\\GPU Engine(*engtype_3D)\\Utilization Percentage");
            if PdhAddEnglishCounterW(query, path, 0, &mut counter) != 0 {
                let _ = PdhCloseQuery(query);
                return None;
            }
            // La primera muestra: el porcentaje sale de la próxima.
            let _ = PdhCollectQueryData(query);
            Some(Gpu { query, counter })
        }
    }
}

/// Suma del uso 3D de cada proceso (lo que muestra el Administrador de
/// tareas). `None` en la primera muestra o si PDH no lo dice.
fn gpu(s: &mut Sampler) -> Option<f32> {
    if s.gpu_off {
        return None;
    }
    let Some(gpu) = s.gpu.as_ref() else {
        s.gpu = Gpu::open();
        s.gpu_off = s.gpu.is_none();
        return None;
    };
    // SAFETY: la consulta y el contador siguen abiertos (los cierra `Drop`);
    // el búfer tiene al menos los bytes que pidió PDH.
    unsafe {
        if PdhCollectQueryData(gpu.query) != 0 {
            return None;
        }
        let (mut size, mut count) = (0u32, 0u32);
        let status = PdhGetFormattedCounterArrayW(gpu.counter, PDH_FMT_DOUBLE, &mut size, &mut count, None);
        if status == 0 {
            // Ningún proceso usa la GPU.
            return Some(0.0);
        }
        if status != PDH_MORE_DATA || size == 0 {
            return None;
        }
        let item = std::mem::size_of::<PDH_FMT_COUNTERVALUE_ITEM_W>();
        // Los nombres van detrás de los ítems, en el mismo búfer.
        let mut buf = vec![PDH_FMT_COUNTERVALUE_ITEM_W::default(); (size as usize).div_ceil(item)];
        let status = PdhGetFormattedCounterArrayW(
            gpu.counter,
            PDH_FMT_DOUBLE,
            &mut size,
            &mut count,
            Some(buf.as_mut_ptr()),
        );
        if status != 0 {
            return None;
        }
        let total: f64 = buf[..(count as usize).min(buf.len())]
            .iter()
            .filter(|i| {
                i.FmtValue.CStatus == PDH_CSTATUS_VALID_DATA || i.FmtValue.CStatus == PDH_CSTATUS_NEW_DATA
            })
            .map(|i| i.FmtValue.Anonymous.doubleValue)
            .filter(|v| v.is_finite())
            .sum();
        Some(total.clamp(0.0, 100.0) as f32)
    }
}

/// Bytes recibidos y enviados por los adaptadores físicos conectados.
fn net_totals() -> Option<(u64, u64)> {
    let mut table: *mut MIB_IF_TABLE2 = std::ptr::null_mut();
    // SAFETY: Windows reserva la tabla; se lee `NumEntries` filas y se libera
    // con `FreeMibTable`.
    unsafe {
        if GetIfTable2(&mut table).0 != 0 || table.is_null() {
            return None;
        }
        let first = std::ptr::addr_of!((*table).Table).cast::<MIB_IF_ROW2>();
        let rows = std::slice::from_raw_parts(first, (*table).NumEntries as usize);
        let (mut down, mut up) = (0u64, 0u64);
        for row in rows {
            let flags = row.InterfaceAndOperStatusFlags._bitfield;
            if row.OperStatus == IfOperStatusUp
                && row.Type != IF_TYPE_SOFTWARE_LOOPBACK
                && flags & IF_HARDWARE != 0
                && flags & IF_FILTER == 0
            {
                down = down.wrapping_add(row.InOctets);
                up = up.wrapping_add(row.OutOctets);
            }
        }
        FreeMibTable(table as *const core::ffi::c_void);
        Some((down, up))
    }
}

/// Las unidades fijas y extraíbles con espacio: la del sistema primero, hasta 4.
fn disks() -> Vec<Disk> {
    let system = std::env::var("SystemDrive")
        .ok()
        .and_then(|d| d.chars().next())
        .map(|c| c.to_ascii_uppercase())
        .unwrap_or('C');
    // Un lector de tarjetas vacío no debe abrir «Inserte un disco».
    let mut old_mode = THREAD_ERROR_MODE::default();
    // SAFETY: solo cambia el modo de errores de este hilo; se restaura abajo.
    let restore = unsafe { SetThreadErrorMode(SEM_FAILCRITICALERRORS, Some(&mut old_mode)) }.is_ok();
    // SAFETY: sin argumentos.
    let mask = unsafe { GetLogicalDrives() };
    let mut out = Vec::new();
    for i in 0..26u32 {
        if mask & (1 << i) == 0 {
            continue;
        }
        let letter = (b'A' + i as u8) as char;
        let root_buf = wide_z(&format!("{letter}:\\"));
        let root = PCWSTR(root_buf.as_ptr());
        // SAFETY: `root_buf` vive hasta el final de la vuelta y termina en 0.
        let kind = unsafe { GetDriveTypeW(root) };
        if kind != DRIVE_FIXED && kind != DRIVE_REMOVABLE {
            continue;
        }
        let (mut free, mut total) = (0u64, 0u64);
        // SAFETY: igual que arriba; los contadores son locales.
        if unsafe { GetDiskFreeSpaceExW(root, Some(&mut free), Some(&mut total), None) }.is_err() || total == 0 {
            continue;
        }
        let mut label_buf = [0u16; 261];
        // SAFETY: igual que arriba; el búfer es local.
        let label = match unsafe { GetVolumeInformationW(root, Some(&mut label_buf[..]), None, None, None, None) } {
            Ok(()) => wide(&label_buf).trim().to_string(),
            Err(_) => String::new(),
        };
        let name = if label.is_empty() { format!("{letter}:") } else { format!("{label} ({letter}:)") };
        out.push((letter != system, letter, Disk { name, free, total, removable: kind == DRIVE_REMOVABLE }));
    }
    if restore {
        // SAFETY: vuelve al modo que tenía el hilo.
        let _ = unsafe { SetThreadErrorMode(old_mode, None) };
    }
    out.sort_by_key(|(other, letter, _)| (*other, *letter));
    out.into_iter().take(4).map(|(_, _, disk)| disk).collect()
}

// --- Batería -----------------------------------------------------------------

/// La primera fila de una consulta WQL.
fn first(services: &IWbemServices, query: &str) -> Option<IWbemClassObject> {
    // SAFETY: llamadas COM sobre un servicio ya conectado.
    unsafe {
        let flags = WBEM_GENERIC_FLAG_TYPE(WBEM_FLAG_FORWARD_ONLY.0 | WBEM_FLAG_RETURN_IMMEDIATELY.0);
        let rows = services.ExecQuery(&BSTR::from("WQL"), &BSTR::from(query), flags, None).ok()?;
        let mut row = [None];
        let mut got = 0u32;
        let _ = rows.Next(WBEM_INFINITE, &mut row, &mut got);
        if got == 0 {
            return None;
        }
        row[0].take()
    }
}

fn field(object: &IWbemClassObject, name: &str) -> Option<VARIANT> {
    let name = wide_z(name);
    let mut value = VARIANT::default();
    // SAFETY: `name` termina en 0 y `value` es local.
    unsafe {
        object.Get(PCWSTR(name.as_ptr()), 0, &mut value, None, None).ok()?;
    }
    Some(value)
}

/// Un número de la primera fila de `class` en ROOT\WMI.
fn wmi_u32(services: &IWbemServices, class: &str, name: &str) -> Option<u32> {
    let object = first(services, &format!("SELECT {name} FROM {class}"))?;
    u32::try_from(&field(&object, name)?).ok()
}

/// Solo si hay batería: salud y ciclos (cada minuto) y watts (cada vez).
fn power(s: &mut Sampler, wmi: Option<&IWbemServices>) -> Option<Power> {
    let mut status = SYSTEM_POWER_STATUS::default();
    // SAFETY: `status` es local.
    let read = unsafe { GetSystemPowerStatus(&mut status) };
    read.ok()?;
    // 128: sin batería (255: no se sabe; tampoco se muestra).
    if status.BatteryFlag & 128 != 0 {
        return None;
    }
    let Some(services) = wmi else {
        return Some(Power::default());
    };
    let fresh = s.battery.as_ref().is_some_and(|(at, ..)| at.elapsed() < BATTERY_EVERY);
    if !fresh {
        s.battery = Some((
            Instant::now(),
            wmi_u32(services, "BatteryStaticData", "DesignedCapacity"),
            wmi_u32(services, "BatteryFullChargedCapacity", "FullChargedCapacity"),
            wmi_u32(services, "BatteryCycleCount", "CycleCount"),
        ));
    }
    let (_, design, full, cycles) = s.battery.unwrap_or((Instant::now(), None, None, None));
    let health = match (design, full) {
        (Some(design), Some(full)) if design > 0 => Some((full as f32 / design as f32).clamp(0.0, 1.0)),
        _ => None,
    };
    let watts = first(services, "SELECT ChargeRate, DischargeRate FROM BatteryStatus").and_then(|row| {
        let rate = |name| field(&row, name).and_then(|v| i32::try_from(&v).ok());
        let (charge, discharge) = (rate("ChargeRate"), rate("DischargeRate"));
        match (charge, discharge) {
            (Some(c), _) if c > 0 => Some(c as f32 / 1000.0),
            (_, Some(d)) if d > 0 => Some(-(d as f32) / 1000.0),
            (None, None) => None,
            _ => Some(0.0),
        }
    });
    Some(Power { health, cycles, watts })
}

// --- Pantallas ---------------------------------------------------------------

fn blank_devmode() -> DEVMODEW {
    DEVMODEW { dmSize: std::mem::size_of::<DEVMODEW>() as u16, ..Default::default() }
}

/// El modo `index` de la pantalla (o el de ahora con `ENUM_CURRENT_SETTINGS`).
fn devmode(device: &[u16], index: ENUM_DISPLAY_SETTINGS_MODE) -> Option<DEVMODEW> {
    let mut dm = blank_devmode();
    // SAFETY: `device` termina en 0 y `dm` dice su tamaño.
    let ok = unsafe { EnumDisplaySettingsExW(PCWSTR(device.as_ptr()), index, &mut dm, ENUM_DISPLAY_SETTINGS_FLAGS(0)) };
    ok.as_bool().then_some(dm)
}

/// Todos los modos que ofrece el driver, en el orden en que los da.
fn each_devmode(device: &[u16]) -> impl Iterator<Item = DEVMODEW> + '_ {
    // El tope evita un driver que no termine nunca.
    (0..4096u32).map_while(move |i| devmode(device, ENUM_DISPLAY_SETTINGS_MODE(i)))
}

fn mode_of(dm: &DEVMODEW) -> Mode {
    Mode {
        width: dm.dmPelsWidth,
        height: dm.dmPelsHeight,
        // 0 y 1: «la predeterminada del hardware».
        hz: if dm.dmDisplayFrequency <= 1 { 0 } else { dm.dmDisplayFrequency },
        hidpi: false,
        key: 0,
    }
}

/// Sin repetir (el driver da cada modo por orientación y escalado) y de la
/// más grande a la más chica.
fn tidy(mut modes: Vec<Mode>) -> Vec<Mode> {
    modes.sort_by(|a, b| (b.width, b.height, b.hz).cmp(&(a.width, a.height, a.hz)));
    modes.dedup_by_key(|m| (m.width, m.height, m.hz));
    modes
}

fn mode_label(mode: &Mode) -> String {
    if mode.hz == 0 {
        format!("{} × {}", mode.width, mode.height)
    } else {
        format!("{} × {} a {} Hz", mode.width, mode.height, mode.hz)
    }
}

pub(crate) fn modes(device: &str) -> (Option<Mode>, Vec<Mode>) {
    let name = wide_z(device);
    let current = devmode(&name, ENUM_CURRENT_SETTINGS).map(|dm| mode_of(&dm));
    let mut all: Vec<Mode> = each_devmode(&name).filter(|dm| dm.dmBitsPerPel == 32).map(|dm| mode_of(&dm)).collect();
    if current.is_none() && all.is_empty() {
        return (None, vec![]);
    }
    if let Some(current) = &current {
        if !all.contains(current) {
            all.push(current.clone());
        }
    }
    (current, tidy(all))
}

fn changed(result: DISP_CHANGE, what: &str) -> Result<(), String> {
    if result == DISP_CHANGE_SUCCESSFUL {
        Ok(())
    } else if result == DISP_CHANGE_RESTART {
        Err(format!("Windows aplica {what} cuando reinicies"))
    } else {
        Err(format!("Windows no aceptó {what} (código {})", result.0))
    }
}

pub(crate) fn set_mode(device: &str, mode: &Mode) -> Result<(), String> {
    let name = wide_z(device);
    let found = each_devmode(&name).find(|dm| dm.dmBitsPerPel == 32 && mode_of(dm) == *mode);
    let mut dm = found.unwrap_or_else(|| {
        let mut dm = blank_devmode();
        dm.dmPelsWidth = mode.width;
        dm.dmPelsHeight = mode.height;
        dm.dmDisplayFrequency = mode.hz;
        dm
    });
    dm.dmBitsPerPel = 32;
    dm.dmFields = DM_BITSPERPEL
        | DM_PELSWIDTH
        | DM_PELSHEIGHT
        | if mode.hz > 0 { DM_DISPLAYFREQUENCY } else { DEVMODE_FIELD_FLAGS(0) };
    // SAFETY: `name` termina en 0 y `dm` vive durante la llamada.
    let result = unsafe { ChangeDisplaySettingsExW(PCWSTR(name.as_ptr()), Some(&dm), None, CDS_UPDATEREGISTRY, None) };
    changed(result, &mode_label(mode))
}

/// Las pantallas que forman el escritorio: (nombre terminado en 0, modo de
/// ahora, si es la principal).
fn attached() -> Vec<([u16; 32], DEVMODEW, bool)> {
    let mut out = Vec::new();
    for i in 0..64u32 {
        let mut dd = DISPLAY_DEVICEW { cb: std::mem::size_of::<DISPLAY_DEVICEW>() as u32, ..Default::default() };
        // SAFETY: `dd` dice su tamaño.
        if !unsafe { EnumDisplayDevicesW(PCWSTR::null(), i, &mut dd, 0) }.as_bool() {
            break;
        }
        if !dd.StateFlags.contains(DISPLAY_DEVICE_ATTACHED_TO_DESKTOP)
            || dd.StateFlags.contains(DISPLAY_DEVICE_MIRRORING_DRIVER)
        {
            continue;
        }
        if let Some(dm) = devmode(&dd.DeviceName, ENUM_CURRENT_SETTINGS) {
            out.push((dd.DeviceName, dm, dd.StateFlags.contains(DISPLAY_DEVICE_PRIMARY_DEVICE)));
        }
    }
    out
}

fn position(dm: &DEVMODEW) -> POINTL {
    // SAFETY: en una pantalla la unión trae `dmPosition`.
    unsafe { dm.Anonymous1.Anonymous2.dmPosition }
}

/// La principal es la que está en (0, 0): se corren todas para que la
/// elegida quede ahí, se guarda cada una sin aplicar y al final se aplica todo
/// junto.
pub(crate) fn set_primary(device: &str) -> Result<(), String> {
    let displays = attached();
    let Some((_, target, primary)) = displays.iter().find(|(name, ..)| wide(name).eq_ignore_ascii_case(device)) else {
        return Err("Windows no encontró esa pantalla".to_string());
    };
    if *primary {
        return Ok(());
    }
    let origin = position(target);
    for (name, mut dm, _) in displays.iter().copied() {
        let at = position(&dm);
        // SAFETY: en una pantalla la unión trae `dmPosition`.
        let mut display = unsafe { dm.Anonymous1.Anonymous2 };
        display.dmPosition = POINTL { x: at.x - origin.x, y: at.y - origin.y };
        dm.Anonymous1 = DEVMODEW_0 { Anonymous2: display };
        dm.dmFields = DM_POSITION;
        let mut flags = CDS_UPDATEREGISTRY | CDS_NORESET;
        if wide(&name).eq_ignore_ascii_case(device) {
            flags = flags | CDS_SET_PRIMARY;
        }
        // SAFETY: `name` termina en 0 y `dm` vive durante la llamada.
        let result = unsafe { ChangeDisplaySettingsExW(PCWSTR(name.as_ptr()), Some(&dm), None, flags, None) };
        changed(result, "la pantalla principal")?;
    }
    // SAFETY: sin pantalla ni modo: aplica lo que quedó guardado.
    let result = unsafe { ChangeDisplaySettingsExW(PCWSTR::null(), None, None, CDS_TYPE(0), None) };
    changed(result, "la pantalla principal")
}

/// Duplicar o extender, como Win + P.
pub(crate) fn set_mirror(on: bool) -> Result<(), String> {
    let topology = if on { SDC_TOPOLOGY_CLONE } else { SDC_TOPOLOGY_EXTEND };
    // SAFETY: sin rutas ni modos: Windows usa los que tiene guardados.
    let code = unsafe { SetDisplayConfig(None, None, SDC_APPLY | topology) };
    if code == 0 {
        Ok(())
    } else if on {
        Err(format!("Windows no pudo duplicar las pantallas (código {code})"))
    } else {
        Err(format!("Windows no pudo extender las pantallas (código {code})"))
    }
}

pub(crate) fn mirrored() -> bool {
    let (mut paths, mut modes) = (0u32, 0u32);
    // SAFETY: los búferes tienen los elementos que dicen los contadores.
    unsafe {
        if GetDisplayConfigBufferSizes(QDC_DATABASE_CURRENT, &mut paths, &mut modes).0 != 0 || paths == 0 {
            return false;
        }
        let mut path_buf = vec![DISPLAYCONFIG_PATH_INFO::default(); paths as usize];
        let mut mode_buf = vec![DISPLAYCONFIG_MODE_INFO::default(); modes.max(1) as usize];
        let mut topology = DISPLAYCONFIG_TOPOLOGY_ID::default();
        let result = QueryDisplayConfig(
            QDC_DATABASE_CURRENT,
            &mut paths,
            path_buf.as_mut_ptr(),
            &mut modes,
            mode_buf.as_mut_ptr(),
            Some(&mut topology),
        );
        result.0 == 0 && topology == DISPLAYCONFIG_TOPOLOGY_CLONE
    }
}

/// Salidas activas: duplicando son dos aunque Windows vea un solo monitor.
pub(crate) fn screens() -> usize {
    let (mut paths, mut modes) = (0u32, 0u32);
    // SAFETY: solo escribe los dos contadores.
    let result = unsafe { GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut paths, &mut modes) };
    if result.0 == 0 { paths as usize } else { 0 }
}

/// El panel «Transmitir» (Win + K).
pub(crate) fn connect() -> Result<(), String> {
    std::process::Command::new("explorer")
        .arg("ms-settings-connectabledevices:devicediscovery")
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("No se pudo abrir Transmitir: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mode(width: u32, height: u32, hz: u32) -> Mode {
        Mode { width, height, hz, hidpi: false, key: 0 }
    }

    #[test]
    fn tidy_ordena_y_no_repite() {
        let got = tidy(vec![mode(1280, 720, 60), mode(1920, 1080, 60), mode(1920, 1080, 144), mode(1920, 1080, 60)]);
        assert_eq!(got, vec![mode(1920, 1080, 144), mode(1920, 1080, 60), mode(1280, 720, 60)]);
    }
}
