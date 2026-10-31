//! Rendimiento y resoluciones en macOS, para el hilo de `os.rs`.
//!
//! - CPU: `host_statistics` entre lecturas (lo mismo que el Monitor de
//!   Actividad), no la suma de `ps`, que es un promedio que decae.
//! - Temperaturas: los sensores HID del chip (Apple Silicon). Es API
//!   privada pero estable (la usan Stats y macmon) y no pide permisos. En un
//!   Mac con Intel no hay y quedan en `None`.
//! - GPU y batería: `ioreg`, que tarda ~15 ms.
//! - Discos: la capacidad «para uso importante», la que muestra el Finder
//!   (cuenta lo purgable como libre).
//! - Pantallas: los modos de CoreGraphics, con los HiDPI (los «parece
//!   1440 × 900» de Ajustes).

use super::os::{Disk, Mode, Power, Vitals};
use std::collections::HashMap;
use std::ffi::c_void;
use std::process::Command;
use std::time::Instant;

type Ref = *const c_void;

const UTF8: u32 = 0x0800_0100;

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFArrayGetCount(array: Ref) -> isize;
    fn CFArrayGetValueAtIndex(array: Ref, index: isize) -> Ref;
    fn CFRelease(object: Ref);
    fn CFStringCreateWithCString(alloc: Ref, text: *const std::ffi::c_char, encoding: u32) -> Ref;
    fn CFStringGetCString(text: Ref, buf: *mut u8, size: isize, encoding: u32) -> u8;
    fn CFNumberCreate(alloc: Ref, kind: isize, value: *const c_void) -> Ref;
    fn CFNumberGetValue(number: Ref, kind: isize, value: *mut c_void) -> u8;
    fn CFBooleanGetValue(boolean: Ref) -> u8;
    fn CFDictionaryCreate(alloc: Ref, keys: *const Ref, values: *const Ref, count: isize, key_callbacks: Ref, value_callbacks: Ref) -> Ref;
    fn CFURLCreateFromFileSystemRepresentation(alloc: Ref, path: *const u8, len: isize, directory: u8) -> Ref;
    fn CFURLCopyResourcePropertyForKey(url: Ref, key: Ref, value: *mut Ref, error: *mut Ref) -> u8;
    static kCFTypeDictionaryKeyCallBacks: u8;
    static kCFTypeDictionaryValueCallBacks: u8;
    static kCFBooleanTrue: Ref;
    static kCFURLVolumeTotalCapacityKey: Ref;
    static kCFURLVolumeAvailableCapacityKey: Ref;
    static kCFURLVolumeAvailableCapacityForImportantUsageKey: Ref;
    static kCFURLVolumeLocalizedNameKey: Ref;
    static kCFURLVolumeIsInternalKey: Ref;
}

#[link(name = "IOKit", kind = "framework")]
extern "C" {
    fn IOHIDEventSystemClientCreate(alloc: Ref) -> Ref;
    fn IOHIDEventSystemClientSetMatching(client: Ref, matching: Ref) -> i32;
    fn IOHIDEventSystemClientCopyServices(client: Ref) -> Ref;
    fn IOHIDServiceClientCopyProperty(service: Ref, key: Ref) -> Ref;
    fn IOHIDServiceClientCopyEvent(service: Ref, kind: i64, options: i32, timestamp: i64) -> Ref;
    fn IOHIDEventGetFloatValue(event: Ref, field: i32) -> f64;
}

// `CGDisplayBounds` también está en `platform/macos/control.rs`, con otro
// `CGRect` del mismo tamaño y orden.
#[allow(clashing_extern_declarations)]
#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGMainDisplayID() -> u32;
    fn CGGetActiveDisplayList(max: u32, displays: *mut u32, count: *mut u32) -> i32;
    fn CGGetOnlineDisplayList(max: u32, displays: *mut u32, count: *mut u32) -> i32;
    fn CGDisplayMirrorsDisplay(display: u32) -> u32;
    fn CGDisplayBounds(display: u32) -> CGRect;
    fn CGDisplayCopyAllDisplayModes(display: u32, options: Ref) -> Ref;
    fn CGDisplayCopyDisplayMode(display: u32) -> Ref;
    fn CGDisplayModeGetWidth(mode: Ref) -> usize;
    fn CGDisplayModeGetHeight(mode: Ref) -> usize;
    fn CGDisplayModeGetPixelWidth(mode: Ref) -> usize;
    fn CGDisplayModeGetRefreshRate(mode: Ref) -> f64;
    fn CGDisplayModeIsUsableForDesktopGUI(mode: Ref) -> bool;
    fn CGDisplayModeGetIODisplayModeID(mode: Ref) -> i32;
    fn CGDisplayModeRelease(mode: Ref);
    fn CGBeginDisplayConfiguration(config: *mut *mut c_void) -> i32;
    fn CGConfigureDisplayWithDisplayMode(config: *mut c_void, display: u32, mode: Ref, options: Ref) -> i32;
    fn CGConfigureDisplayOrigin(config: *mut c_void, display: u32, x: i32, y: i32) -> i32;
    fn CGConfigureDisplayMirrorOfDisplay(config: *mut c_void, display: u32, master: u32) -> i32;
    fn CGCompleteDisplayConfiguration(config: *mut c_void, option: u32) -> i32;
    fn CGCancelDisplayConfiguration(config: *mut c_void) -> i32;
    static kCGDisplayShowDuplicateLowResolutionModes: Ref;
}

#[link(name = "System")]
extern "C" {
    fn mach_host_self() -> u32;
    fn host_statistics(host: u32, flavor: i32, info: *mut i32, count: *mut u32) -> i32;
}

#[repr(C)]
#[derive(Clone, Copy)]
struct CGRect {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

/// Que el cambio quede guardado (el que usa Ajustes).
const CONFIGURE_PERMANENTLY: u32 = 2;

fn cfstr(text: &str) -> Ref {
    let text = std::ffi::CString::new(text).unwrap_or_default();
    // SAFETY: cadena terminada en 0; el llamador libera el resultado.
    unsafe { CFStringCreateWithCString(std::ptr::null(), text.as_ptr(), UTF8) }
}

fn cf_string(text: Ref) -> Option<String> {
    if text.is_null() {
        return None;
    }
    let mut buf = [0u8; 256];
    // SAFETY: búfer de 256; `text` es un CFString vivo.
    unsafe {
        (CFStringGetCString(text, buf.as_mut_ptr(), buf.len() as isize, UTF8) != 0)
            .then(|| std::ffi::CStr::from_ptr(buf.as_ptr().cast()).to_string_lossy().into_owned())
    }
}

// --- Muestras entre lecturas ---------------------------------------------------

/// Lo que se recuerda de la lectura anterior.
#[derive(Default)]
pub struct Sampler {
    /// Ticks ocupados y totales de la CPU.
    cpu: Option<(u64, u64)>,
    /// Los contadores de cada interfaz son de 32 bits: se suman las
    /// diferencias para no ver un salto cada 4 GB.
    ifaces: HashMap<String, (u32, u32)>,
    net_total: (u64, u64),
    net: Option<(u64, u64, Instant)>,
    hid: Option<Hid>,
}

pub fn vitals(s: &mut Sampler) -> Vitals {
    let (cpu_temp, gpu_temp) = temperatures(s);
    let (down, up) = net_counters(s);
    let (net_down, net_up) = super::os::net_rate(&mut s.net, down, up);
    Vitals {
        cpu: cpu(s),
        cpu_temp,
        gpu: gpu(),
        gpu_temp,
        net_down,
        net_up,
        disks: disks(),
        thermal: Some(thermal_state()),
        power: power(),
        uptime: uptime(),
        // La memoria la pone `os.rs`, que ya la lee para Procesos.
        ..Vitals::default()
    }
}

fn cpu(s: &mut Sampler) -> f32 {
    const HOST_CPU_LOAD_INFO: i32 = 3;
    static HOST: std::sync::OnceLock<u32> = std::sync::OnceLock::new();
    // Usuario, sistema, reposo y nice.
    let mut ticks = [0i32; 4];
    let mut count = 4u32;
    // SAFETY: `ticks` tiene los 4 enteros que pide `HOST_CPU_LOAD_INFO`.
    let ok = unsafe {
        host_statistics(*HOST.get_or_init(|| mach_host_self()), HOST_CPU_LOAD_INFO, ticks.as_mut_ptr(), &mut count)
    } == 0;
    if !ok {
        return 0.0;
    }
    let ticks = ticks.map(|t| t as u32 as u64);
    let busy = ticks[0] + ticks[1] + ticks[3];
    let total = busy + ticks[2];
    let pct = match s.cpu {
        Some((b0, t0)) if total > t0 => busy.saturating_sub(b0) as f32 / (total - t0) as f32 * 100.0,
        _ => 0.0,
    };
    s.cpu = Some((busy, total));
    pct.clamp(0.0, 100.0)
}

// --- Temperaturas ----------------------------------------------------------------

/// El cliente de los sensores, creado una vez.
struct Hid {
    client: Ref,
    product: Ref,
}

// SAFETY: solo lo usa el hilo de Sistema.
unsafe impl Send for Hid {}

fn hid() -> Option<Hid> {
    // SAFETY: CoreFoundation con objetos propios; se liberan los de paso.
    unsafe {
        let (page, usage) = (0xff00i32, 5i32);
        let keys = [cfstr("PrimaryUsagePage"), cfstr("PrimaryUsage")];
        let values = [
            CFNumberCreate(std::ptr::null(), 3, &page as *const i32 as *const c_void),
            CFNumberCreate(std::ptr::null(), 3, &usage as *const i32 as *const c_void),
        ];
        let matching = CFDictionaryCreate(
            std::ptr::null(),
            keys.as_ptr(),
            values.as_ptr(),
            2,
            &kCFTypeDictionaryKeyCallBacks as *const u8 as Ref,
            &kCFTypeDictionaryValueCallBacks as *const u8 as Ref,
        );
        for object in keys.into_iter().chain(values) {
            CFRelease(object);
        }
        let client = IOHIDEventSystemClientCreate(std::ptr::null());
        if client.is_null() {
            CFRelease(matching);
            return None;
        }
        IOHIDEventSystemClientSetMatching(client, matching);
        CFRelease(matching);
        Some(Hid { client, product: cfstr("Product") })
    }
}

/// CPU (promedio de los núcleos) y GPU (el sensor más caliente).
fn temperatures(s: &mut Sampler) -> (Option<f32>, Option<f32>) {
    if s.hid.is_none() {
        s.hid = hid();
    }
    let Some(hid) = s.hid.as_ref() else {
        return (None, None);
    };
    const TEMPERATURE: i64 = 15;
    let mut readings = Vec::new();
    // SAFETY: los servicios y eventos copiados se liberan aquí mismo.
    unsafe {
        let services = IOHIDEventSystemClientCopyServices(hid.client);
        if services.is_null() {
            return (None, None);
        }
        for i in 0..CFArrayGetCount(services) {
            let service = CFArrayGetValueAtIndex(services, i);
            let name = IOHIDServiceClientCopyProperty(service, hid.product);
            let label = cf_string(name);
            if !name.is_null() {
                CFRelease(name);
            }
            let event = IOHIDServiceClientCopyEvent(service, TEMPERATURE, 0, 0);
            if event.is_null() {
                continue;
            }
            let value = IOHIDEventGetFloatValue(event, (TEMPERATURE as i32) << 16) as f32;
            CFRelease(event);
            if let Some(label) = label {
                readings.push((label, value));
            }
        }
        CFRelease(services);
    }
    summarize_temps(&readings)
}

/// De los sensores con nombre, la CPU y la GPU. Los núcleos se llaman
/// «pACC/eACC MTR Temp Sensor» en M1 y «PMU tdie» en los demás.
pub(super) fn summarize_temps(readings: &[(String, f32)]) -> (Option<f32>, Option<f32>) {
    let sane = |v: f32| (5.0..130.0).contains(&v);
    let average = |pick: &dyn Fn(&str) -> bool| {
        let values: Vec<f32> = readings.iter().filter(|(n, v)| pick(n) && sane(*v)).map(|(_, v)| *v).collect();
        (!values.is_empty()).then(|| values.iter().sum::<f32>() / values.len() as f32)
    };
    let cpu = average(&|n| n.contains("ACC MTR Temp")).or_else(|| average(&|n| n.contains("tdie")));
    let gpu = readings
        .iter()
        .filter(|(n, v)| n.starts_with("GPU MTR Temp") && sane(*v))
        .map(|(_, v)| *v)
        .max_by(f32::total_cmp);
    (cpu, gpu)
}

// --- GPU, batería y estado térmico ---------------------------------------------------

fn ioreg(class: &str, depth: bool) -> String {
    let mut command = Command::new("ioreg");
    command.args(["-r", "-c", class, "-w", "0"]);
    if depth {
        command.args(["-d", "1"]);
    }
    command.output().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default()
}

/// El primer `"clave" = número` (o `"clave"=número`, dentro de un
/// diccionario) de la salida de `ioreg`.
pub(super) fn ioreg_number(text: &str, key: &str) -> Option<i64> {
    let quoted = format!("\"{key}\"");
    let mut rest = text;
    while let Some(at) = rest.find(&quoted) {
        let after = rest[at + quoted.len()..].trim_start();
        if let Some(value) = after.strip_prefix('=') {
            let digits: String = value
                .trim_start()
                .chars()
                .take_while(|c| c.is_ascii_digit() || *c == '-')
                .collect();
            // Los negativos llegan como su complemento a dos sin signo.
            if let Ok(n) = digits.parse::<i64>() {
                return Some(n);
            }
            if let Ok(n) = digits.parse::<u64>() {
                return Some(n as i64);
            }
        }
        rest = &rest[at + quoted.len()..];
    }
    None
}

fn gpu() -> Option<f32> {
    ioreg_number(&ioreg("IOAccelerator", true), "Device Utilization %").map(|n| n.clamp(0, 100) as f32)
}

fn power() -> Option<Power> {
    let text = ioreg("AppleSmartBattery", false);
    parse_power(&text)
}

pub(super) fn parse_power(text: &str) -> Option<Power> {
    let design = ioreg_number(text, "DesignCapacity")?;
    let full = ioreg_number(text, "AppleRawMaxCapacity").or_else(|| ioreg_number(text, "NominalChargeCapacity"));
    let health = full.filter(|_| design > 0).map(|f| (f as f32 / design as f32).clamp(0.0, 1.0));
    let watts = match (ioreg_number(text, "Voltage"), ioreg_number(text, "InstantAmperage").or_else(|| ioreg_number(text, "Amperage"))) {
        (Some(mv), Some(ma)) if mv > 0 => Some(mv as f32 * ma as f32 / 1_000_000.0),
        _ => None,
    };
    Some(Power {
        health,
        cycles: ioreg_number(text, "CycleCount").map(|n| n.max(0) as u32),
        watts,
    })
}

fn thermal_state() -> u8 {
    use objc2::{class, msg_send};
    // SAFETY: `processInfo` y `thermalState` son de solo lectura.
    unsafe {
        let info: *mut objc2::runtime::AnyObject = msg_send![class!(NSProcessInfo), processInfo];
        let state: isize = msg_send![info, thermalState];
        state.clamp(0, 3) as u8
    }
}

fn uptime() -> u64 {
    let text = Command::new("sysctl").args(["-n", "kern.boottime"]).output().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
    // «{ sec = 1793318165, usec = 693770 } Thu Oct 29 20:56:05 2026»
    let boot = text
        .split("sec = ")
        .nth(1)
        .and_then(|r| r.split(',').next())
        .and_then(|n| n.trim().parse::<u64>().ok());
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    boot.map_or(0, |b| now.saturating_sub(b))
}

// --- Red y discos -------------------------------------------------------------------

/// Bytes recibidos y enviados desde que se encendió, por Wi-Fi, Ethernet y
/// datos del teléfono. Sin las VPN (`utun`): repetirían lo mismo.
fn net_counters(s: &mut Sampler) -> (u64, u64) {
    let mut seen = HashMap::new();
    // SAFETY: se recorre la lista de `getifaddrs` y se libera al final.
    unsafe {
        let mut list: *mut libc::ifaddrs = std::ptr::null_mut();
        if libc::getifaddrs(&mut list) != 0 {
            return s.net_total;
        }
        let mut cursor = list;
        while !cursor.is_null() {
            let entry = &*cursor;
            cursor = entry.ifa_next;
            if entry.ifa_addr.is_null() || (*entry.ifa_addr).sa_family as i32 != libc::AF_LINK || entry.ifa_data.is_null() {
                continue;
            }
            let name = std::ffi::CStr::from_ptr(entry.ifa_name).to_string_lossy().into_owned();
            if !(name.starts_with("en") || name.starts_with("pdp_ip")) {
                continue;
            }
            let data = &*(entry.ifa_data as *const libc::if_data);
            seen.insert(name, (data.ifi_ibytes, data.ifi_obytes));
        }
        libc::freeifaddrs(list);
    }
    for (name, (down, up)) in &seen {
        if let Some((d0, u0)) = s.ifaces.get(name) {
            s.net_total.0 += down.wrapping_sub(*d0) as u64;
            s.net_total.1 += up.wrapping_sub(*u0) as u64;
        }
    }
    s.ifaces = seen;
    s.net_total
}

fn volume(path: &str) -> Option<Disk> {
    // SAFETY: la URL y los valores copiados se liberan aquí mismo.
    unsafe {
        let url = CFURLCreateFromFileSystemRepresentation(std::ptr::null(), path.as_ptr(), path.len() as isize, 1);
        if url.is_null() {
            return None;
        }
        let get = |key: Ref| {
            let mut value: Ref = std::ptr::null();
            (CFURLCopyResourcePropertyForKey(url, key, &mut value, std::ptr::null_mut()) != 0 && !value.is_null()).then_some(value)
        };
        let number = |key: Ref| {
            get(key).and_then(|value| {
                let mut n = 0i64;
                let ok = CFNumberGetValue(value, 4, &mut n as *mut i64 as *mut c_void) != 0;
                CFRelease(value);
                ok.then_some(n.max(0) as u64)
            })
        };
        let total = number(kCFURLVolumeTotalCapacityKey);
        let free = number(kCFURLVolumeAvailableCapacityForImportantUsageKey)
            .filter(|n| *n > 0)
            .or_else(|| number(kCFURLVolumeAvailableCapacityKey));
        let name = get(kCFURLVolumeLocalizedNameKey).and_then(|value| {
            let text = cf_string(value);
            CFRelease(value);
            text
        });
        let internal = get(kCFURLVolumeIsInternalKey).map(|value| {
            let yes = CFBooleanGetValue(value) != 0;
            CFRelease(value);
            yes
        });
        CFRelease(url);
        let total = total.filter(|t| *t > 0)?;
        Some(Disk {
            name: name.unwrap_or_else(|| path.to_string()),
            free: free.unwrap_or(0).min(total),
            total,
            removable: internal == Some(false),
        })
    }
}

/// El disco del sistema y los que están en `/Volumes` (sin el alias del
/// mismo disco del sistema, ni imágenes de instaladores ocultas).
fn disks() -> Vec<Disk> {
    let mut out: Vec<Disk> = volume("/").into_iter().collect();
    if let Ok(entries) = std::fs::read_dir("/Volumes") {
        let mut extra: Vec<Disk> = entries
            .flatten()
            .filter(|e| e.file_type().is_ok_and(|t| t.is_dir() && !t.is_symlink()))
            .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
            .filter_map(|e| volume(&e.path().to_string_lossy()))
            // Las imágenes de instaladores ocupan 100 %; no son un disco.
            .filter(|d| d.total >= 1 << 30)
            .collect();
        extra.sort_by(|a, b| a.name.cmp(&b.name));
        out.extend(extra.into_iter().take(3));
    }
    out
}

// --- Pantallas ---------------------------------------------------------------------

fn all_modes(display: u32) -> Ref {
    // SAFETY: diccionario de una clave con valores constantes de CF.
    unsafe {
        let keys = [kCGDisplayShowDuplicateLowResolutionModes];
        let values = [kCFBooleanTrue];
        let options = CFDictionaryCreate(
            std::ptr::null(),
            keys.as_ptr(),
            values.as_ptr(),
            1,
            &kCFTypeDictionaryKeyCallBacks as *const u8 as Ref,
            &kCFTypeDictionaryValueCallBacks as *const u8 as Ref,
        );
        let modes = CGDisplayCopyAllDisplayModes(display, options);
        CFRelease(options);
        modes
    }
}

fn mode_of(mode: Ref) -> Mode {
    // SAFETY: `mode` es un CGDisplayMode vivo.
    unsafe {
        let width = CGDisplayModeGetWidth(mode) as u32;
        Mode {
            width,
            height: CGDisplayModeGetHeight(mode) as u32,
            hz: CGDisplayModeGetRefreshRate(mode).round() as u32,
            hidpi: CGDisplayModeGetPixelWidth(mode) as u32 > width,
            key: CGDisplayModeGetIODisplayModeID(mode) as i64,
        }
    }
}

/// El modo de ahora y los que se pueden elegir (los que sirven para el
/// escritorio, sin repetir).
pub fn modes(display: u32) -> (Option<Mode>, Vec<Mode>) {
    // SAFETY: se libera el modo actual y la lista copiada.
    unsafe {
        let current = CGDisplayCopyDisplayMode(display);
        let now = (!current.is_null()).then(|| mode_of(current));
        if !current.is_null() {
            CGDisplayModeRelease(current);
        }
        let list = all_modes(display);
        if list.is_null() {
            return (now, Vec::new());
        }
        let mut out: Vec<Mode> = Vec::new();
        for i in 0..CFArrayGetCount(list) {
            let mode = CFArrayGetValueAtIndex(list, i);
            if !CGDisplayModeIsUsableForDesktopGUI(mode) {
                continue;
            }
            let m = mode_of(mode);
            if !out.iter().any(|o| o.width == m.width && o.height == m.height && o.hz == m.hz && o.hidpi == m.hidpi) {
                out.push(m);
            }
        }
        CFRelease(list);
        (now, out)
    }
}

/// Una configuración de pantallas: se arma con `f` y se guarda, o se
/// cancela si algo falló.
fn configure(f: impl FnOnce(*mut c_void) -> Result<(), String>) -> Result<(), String> {
    let mut config: *mut c_void = std::ptr::null_mut();
    // SAFETY: `config` vive hasta completarla o cancelarla.
    unsafe {
        if CGBeginDisplayConfiguration(&mut config) != 0 {
            return Err(crate::i18n::t("pill.system.displayBusy").to_string());
        }
        match f(config) {
            Ok(()) => match CGCompleteDisplayConfiguration(config, CONFIGURE_PERMANENTLY) {
                0 => Ok(()),
                code => Err(crate::i18n::tf("pill.system.displayFailed", &[("code", &code)])),
            },
            Err(error) => {
                CGCancelDisplayConfiguration(config);
                Err(error)
            }
        }
    }
}

pub fn set_mode(display: u32, want: &Mode) -> Result<(), String> {
    let list = all_modes(display);
    if list.is_null() {
        return Err(crate::i18n::t("pill.system.modeMissing").to_string());
    }
    // SAFETY: el modo elegido vive mientras viva la lista, que se libera al final.
    let result = unsafe {
        let found = (0..CFArrayGetCount(list)).map(|i| CFArrayGetValueAtIndex(list, i)).find(|&mode| {
            let m = mode_of(mode);
            m.key == want.key && m.width == want.width && m.height == want.height && m.hidpi == want.hidpi
        });
        match found {
            Some(mode) => configure(|config| {
                match CGConfigureDisplayWithDisplayMode(config, display, mode, std::ptr::null()) {
                    0 => Ok(()),
                    code => Err(crate::i18n::tf("pill.system.displayFailed", &[("code", &code)])),
                }
            }),
            None => Err(crate::i18n::t("pill.system.modeMissing").to_string()),
        }
    };
    // SAFETY: copiada arriba.
    unsafe { CFRelease(list) };
    result
}

fn list(online: bool) -> Vec<u32> {
    let mut ids = [0u32; 16];
    let mut count = 0u32;
    // SAFETY: búfer de 16 ids.
    let ok = unsafe {
        if online {
            CGGetOnlineDisplayList(ids.len() as u32, ids.as_mut_ptr(), &mut count)
        } else {
            CGGetActiveDisplayList(ids.len() as u32, ids.as_mut_ptr(), &mut count)
        }
    } == 0;
    if ok { ids[..count as usize].to_vec() } else { Vec::new() }
}

/// La principal es la que está en (0, 0): se corren todas para que esta
/// quede ahí, sin cambiar cómo están ordenadas entre sí.
pub fn set_primary(display: u32) -> Result<(), String> {
    // SAFETY: consultas puras.
    let origin = unsafe { CGDisplayBounds(display) };
    if unsafe { CGMainDisplayID() } == display {
        return Ok(());
    }
    configure(|config| {
        for id in list(false) {
            // SAFETY: `config` está abierta.
            let bounds = unsafe { CGDisplayBounds(id) };
            let (x, y) = ((bounds.x - origin.x).round() as i32, (bounds.y - origin.y).round() as i32);
            if unsafe { CGConfigureDisplayOrigin(config, id, x, y) } != 0 {
                return Err(crate::i18n::t("pill.system.primaryFailed").to_string());
            }
        }
        Ok(())
    })
}

pub fn set_mirror(on: bool) -> Result<(), String> {
    // SAFETY: consulta pura.
    let main = unsafe { CGMainDisplayID() };
    configure(|config| {
        for id in list(true) {
            if id == main {
                continue;
            }
            // `kCGNullDirectDisplay` (0) deja de duplicar.
            let master = if on { main } else { 0 };
            // SAFETY: `config` está abierta.
            if unsafe { CGConfigureDisplayMirrorOfDisplay(config, id, master) } != 0 {
                return Err(crate::i18n::t("pill.system.mirrorFailed").to_string());
            }
        }
        Ok(())
    })
}

pub fn mirrored() -> bool {
    // SAFETY: consulta pura.
    list(true).into_iter().any(|id| unsafe { CGDisplayMirrorsDisplay(id) } != 0)
}

pub fn screens() -> usize {
    list(true).len()
}

/// Ajustes → Pantallas, donde está «Duplicar o extender a» (AirPlay y
/// Sidecar).
pub fn connect() -> Result<(), String> {
    Command::new("open")
        .arg("x-apple.systempreferences:com.apple.Displays-Settings.extension")
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}
