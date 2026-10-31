//! Lo poco de Core Audio que usa la pill (Sistema, privacidad, medios), con
//! `AudioObjectGetPropertyData` y compañía a mano: el volumen, el silencio y
//! las salidas, qué dispositivo está en uso y qué procesos tienen audio.

use std::ffi::c_void;

pub type ObjectId = u32;

pub const SYSTEM: ObjectId = 1;

const fn four_cc(code: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*code)
}

pub const SCOPE_GLOBAL: u32 = four_cc(b"glob");
pub const SCOPE_INPUT: u32 = four_cc(b"inpt");
pub const SCOPE_OUTPUT: u32 = four_cc(b"outp");
pub const ELEMENT_MAIN: u32 = 0;

pub const DEFAULT_OUTPUT: u32 = four_cc(b"dOut");
pub const DEFAULT_INPUT: u32 = four_cc(b"dIn ");
pub const DEVICES: u32 = four_cc(b"dev#");
pub const PROCESSES: u32 = four_cc(b"prs#");
pub const NAME: u32 = four_cc(b"lnam");
pub const UID: u32 = four_cc(b"uid ");
pub const STREAMS: u32 = four_cc(b"stm#");
pub const RUNNING_SOMEWHERE: u32 = four_cc(b"gone");
pub const VOLUME_SCALAR: u32 = four_cc(b"volm");
/// `kAudioHardwareServiceDeviceProperty_VirtualMainVolume`: el volumen de la
/// barra de menús, aunque el dispositivo no tenga uno general.
pub const VIRTUAL_MAIN_VOLUME: u32 = four_cc(b"vmvc");
pub const MUTE: u32 = four_cc(b"mute");
pub const TRANSPORT: u32 = four_cc(b"tran");
pub const PROCESS_PID: u32 = four_cc(b"ppid");
pub const PROCESS_RUNNING_INPUT: u32 = four_cc(b"piri");
pub const PROCESS_RUNNING_OUTPUT: u32 = four_cc(b"piro");
pub const PROCESS_BUNDLE: u32 = four_cc(b"pbid");

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Address {
    pub selector: u32,
    pub scope: u32,
    pub element: u32,
}

impl Address {
    pub const fn new(selector: u32, scope: u32) -> Self {
        Self {
            selector,
            scope,
            element: ELEMENT_MAIN,
        }
    }
}

#[link(name = "CoreAudio", kind = "framework")]
extern "C" {
    fn AudioObjectHasProperty(object: ObjectId, address: *const Address) -> u8;
    fn AudioObjectIsPropertySettable(object: ObjectId, address: *const Address, settable: *mut u8) -> i32;
    fn AudioObjectGetPropertyDataSize(
        object: ObjectId,
        address: *const Address,
        qualifier_size: u32,
        qualifier: *const c_void,
        size: *mut u32,
    ) -> i32;
    fn AudioObjectGetPropertyData(
        object: ObjectId,
        address: *const Address,
        qualifier_size: u32,
        qualifier: *const c_void,
        size: *mut u32,
        data: *mut c_void,
    ) -> i32;
    fn AudioObjectSetPropertyData(
        object: ObjectId,
        address: *const Address,
        qualifier_size: u32,
        qualifier: *const c_void,
        size: u32,
        data: *const c_void,
    ) -> i32;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFStringGetCString(string: *const c_void, buffer: *mut u8, size: isize, encoding: u32) -> u8;
    fn CFRelease(object: *const c_void);
}

pub fn has(object: ObjectId, address: Address) -> bool {
    // SAFETY: consulta pura.
    unsafe { AudioObjectHasProperty(object, &address) != 0 }
}

pub fn settable(object: ObjectId, address: Address) -> bool {
    let mut out = 0u8;
    // SAFETY: `out` vive durante la llamada.
    unsafe { AudioObjectIsPropertySettable(object, &address, &mut out) == 0 && out != 0 }
}

/// Un valor de tamaño fijo (`u32`, `f32`, `ObjectId`…).
pub fn get<T: Copy + Default>(object: ObjectId, address: Address) -> Option<T> {
    let mut value = T::default();
    let mut size = std::mem::size_of::<T>() as u32;
    // SAFETY: `value` tiene el tamaño que se anuncia.
    let status = unsafe {
        AudioObjectGetPropertyData(object, &address, 0, std::ptr::null(), &mut size, (&mut value as *mut T).cast())
    };
    (status == 0).then_some(value)
}

pub fn set<T: Copy>(object: ObjectId, address: Address, value: T) -> bool {
    // SAFETY: `value` tiene el tamaño que se anuncia.
    unsafe {
        AudioObjectSetPropertyData(
            object,
            &address,
            0,
            std::ptr::null(),
            std::mem::size_of::<T>() as u32,
            (&value as *const T).cast(),
        ) == 0
    }
}

/// Una lista de ids (dispositivos, procesos, flujos).
pub fn list(object: ObjectId, address: Address) -> Vec<ObjectId> {
    let mut size = 0u32;
    // SAFETY: primero el tamaño y después los datos en un búfer de ese tamaño.
    unsafe {
        if AudioObjectGetPropertyDataSize(object, &address, 0, std::ptr::null(), &mut size) != 0 || size == 0 {
            return Vec::new();
        }
        let mut ids = vec![0u32; size as usize / std::mem::size_of::<ObjectId>()];
        if AudioObjectGetPropertyData(object, &address, 0, std::ptr::null(), &mut size, ids.as_mut_ptr().cast()) != 0 {
            return Vec::new();
        }
        ids.truncate(size as usize / std::mem::size_of::<ObjectId>());
        ids
    }
}

/// Un texto (`CFString`, que el llamador suelta).
pub fn string(object: ObjectId, address: Address) -> Option<String> {
    let mut value: *const c_void = std::ptr::null();
    let mut size = std::mem::size_of::<*const c_void>() as u32;
    // SAFETY: Core Audio devuelve un `CFStringRef` +1, que se suelta aquí.
    unsafe {
        let status =
            AudioObjectGetPropertyData(object, &address, 0, std::ptr::null(), &mut size, (&mut value as *mut *const c_void).cast());
        if status != 0 || value.is_null() {
            return None;
        }
        let mut buffer = vec![0u8; 1024];
        // `kCFStringEncodingUTF8`.
        let ok = CFStringGetCString(value, buffer.as_mut_ptr(), buffer.len() as isize, 0x0800_0100);
        CFRelease(value);
        if ok == 0 {
            return None;
        }
        let end = buffer.iter().position(|b| *b == 0).unwrap_or(buffer.len());
        Some(String::from_utf8_lossy(&buffer[..end]).into_owned())
    }
}

/// El dispositivo por omisión de salida o entrada.
pub fn default_device(input: bool) -> Option<ObjectId> {
    let selector = if input { DEFAULT_INPUT } else { DEFAULT_OUTPUT };
    get::<ObjectId>(SYSTEM, Address::new(selector, SCOPE_GLOBAL)).filter(|id| *id != 0)
}

/// Si el dispositivo tiene flujos en ese sentido (para listar salidas).
pub fn has_streams(device: ObjectId, scope: u32) -> bool {
    !list(device, Address::new(STREAMS, scope)).is_empty()
}

/// Los procesos que tienen audio abierto, con su pid y si graban o suenan
/// (macOS 14.2 en adelante; antes, vacío).
pub struct Process {
    pub pid: i32,
    pub bundle: Option<String>,
    pub input: bool,
    pub output: bool,
}

pub fn processes() -> Vec<Process> {
    list(SYSTEM, Address::new(PROCESSES, SCOPE_GLOBAL))
        .into_iter()
        .filter_map(|id| {
            let pid = get::<i32>(id, Address::new(PROCESS_PID, SCOPE_GLOBAL))?;
            Some(Process {
                pid,
                bundle: string(id, Address::new(PROCESS_BUNDLE, SCOPE_GLOBAL)).filter(|b| !b.is_empty()),
                input: get::<u32>(id, Address::new(PROCESS_RUNNING_INPUT, SCOPE_GLOBAL)).is_some_and(|v| v != 0),
                output: get::<u32>(id, Address::new(PROCESS_RUNNING_OUTPUT, SCOPE_GLOBAL)).is_some_and(|v| v != 0),
            })
        })
        .collect()
}
