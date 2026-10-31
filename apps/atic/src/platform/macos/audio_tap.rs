//! Process taps de Core Audio (macOS 14.2+): el nivel real de lo que suena,
//! para la onda de la pill (`Meter`), y el volumen por app de Sistema
//! (`Mixer`).
//!
//! Piden el permiso «Grabación de audio del sistema» (TCC `AudioCapture`,
//! distinto del de pantalla): sin él el IOProc nunca se llama. `Meter` lo
//! nota porque no corren ciclos y quien lo usa vuelve al vaivén.
//!
//! Probado en un Mac (Darwin 27): crear tap, agregado e IOProc da `noErr`, y el
//! tap global entrega float32 estéreo a la tasa de la salida.

use std::ffi::c_void;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;

use objc2::msg_send;
use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyClass, AnyObject, Bool};
use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSString};

use super::coreaudio as ca;

type Status = i32;

const fn four_cc(code: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*code)
}

const TAP_UID: u32 = four_cc(b"tuid");
const TAP_FORMAT: u32 = four_cc(b"tfmt");
const TRANSLATE_PID: u32 = four_cc(b"id2p");
const FORMAT_LPCM: u32 = four_cc(b"lpcm");
const FLAG_FLOAT: u32 = 1;
/// `CATapMuteBehavior`.
const UNMUTED: isize = 0;
const MUTED_WHEN_TAPPED: isize = 2;
/// Prefijo del UID de nuestros agregados: la lista de salidas los esconde.
pub const AGGREGATE_PREFIX: &str = "cl.ciat.atic.";
const MAX_TAPS: usize = 16;

#[repr(C)]
struct AudioBuffer {
    channels: u32,
    bytes: u32,
    data: *mut c_void,
}

#[repr(C)]
struct AudioBufferList {
    count: u32,
    buffers: [AudioBuffer; 1],
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct StreamDescription {
    sample_rate: f64,
    format: u32,
    flags: u32,
    bytes_per_packet: u32,
    frames_per_packet: u32,
    bytes_per_frame: u32,
    channels: u32,
    bits: u32,
    reserved: u32,
}

type IoProc = unsafe extern "C" fn(
    device: ca::ObjectId,
    now: *const c_void,
    input: *const AudioBufferList,
    input_time: *const c_void,
    output: *mut AudioBufferList,
    output_time: *const c_void,
    client: *mut c_void,
) -> Status;
type IoProcId = *mut c_void;

#[link(name = "CoreAudio", kind = "framework")]
extern "C" {
    fn AudioHardwareCreateProcessTap(description: *const AnyObject, tap: *mut ca::ObjectId) -> Status;
    fn AudioHardwareDestroyProcessTap(tap: ca::ObjectId) -> Status;
    fn AudioHardwareCreateAggregateDevice(description: *const c_void, device: *mut ca::ObjectId) -> Status;
    fn AudioHardwareDestroyAggregateDevice(device: ca::ObjectId) -> Status;
    fn AudioDeviceCreateIOProcID(device: ca::ObjectId, proc_: IoProc, client: *mut c_void, out: *mut IoProcId) -> Status;
    fn AudioDeviceDestroyIOProcID(device: ca::ObjectId, id: IoProcId) -> Status;
    fn AudioDeviceStart(device: ca::ObjectId, id: IoProcId) -> Status;
    fn AudioDeviceStop(device: ca::ObjectId, id: IoProcId) -> Status;
    fn AudioObjectGetPropertyData(
        object: ca::ObjectId,
        address: *const ca::Address,
        qualifier_size: u32,
        qualifier: *const c_void,
        size: *mut u32,
        data: *mut c_void,
    ) -> Status;
}

#[derive(Debug)]
pub enum Error {
    /// macOS anterior a 14.2: no existe `CATapDescription`.
    Unsupported,
    /// El tap no entrega float32 (no debería pasar con una mezcla estéreo).
    Format,
    Status(&'static str, Status),
}

/// Para armar diccionarios y listas de Foundation.
fn obj(x: &AnyObject) -> &AnyObject {
    x
}

/// El objeto de proceso de Core Audio de un pid.
fn process_object(pid: i32) -> Option<ca::ObjectId> {
    let address = ca::Address::new(TRANSLATE_PID, ca::SCOPE_GLOBAL);
    let mut object: ca::ObjectId = 0;
    let mut size = std::mem::size_of::<ca::ObjectId>() as u32;
    // SAFETY: el qualifier es el pid y `object` tiene el tamaño anunciado.
    let status = unsafe {
        AudioObjectGetPropertyData(
            ca::SYSTEM,
            &address,
            std::mem::size_of::<i32>() as u32,
            (&pid as *const i32).cast(),
            &mut size,
            (&mut object as *mut ca::ObjectId).cast(),
        )
    };
    (status == 0 && object != 0).then_some(object)
}

/// Un `CATapDescription` privado. `global`: todos los procesos menos
/// `processes`; si no, la mezcla estéreo de `processes`.
fn describe(name: &str, processes: &[ca::ObjectId], global: bool, mute: isize) -> Option<Retained<AnyObject>> {
    let class = AnyClass::get(c"CATapDescription")?;
    let ids: Vec<Retained<NSNumber>> = processes.iter().map(|id| NSNumber::new_u32(*id)).collect();
    let ids = NSArray::from_retained_slice(&ids);
    // SAFETY: selectores de `CATapDescription.h`; los argumentos viven durante
    // la llamada.
    unsafe {
        let alloc: Allocated<AnyObject> = msg_send![class, alloc];
        let desc: Option<Retained<AnyObject>> = if global {
            msg_send![alloc, initStereoGlobalTapButExcludeProcesses: &*ids]
        } else {
            msg_send![alloc, initStereoMixdownOfProcesses: &*ids]
        };
        let desc = desc?;
        let _: () = msg_send![&desc, setName: &*NSString::from_str(name)];
        let _: () = msg_send![&desc, setPrivate: Bool::YES];
        let _: () = msg_send![&desc, setMuteBehavior: mute];
        Some(desc)
    }
}

struct Tap(ca::ObjectId);

impl Tap {
    fn create(desc: &AnyObject) -> Result<Self, Error> {
        let mut id = 0;
        // SAFETY: `desc` es un `CATapDescription` vivo.
        let status = unsafe { AudioHardwareCreateProcessTap(desc, &mut id) };
        if status != 0 || id == 0 {
            return Err(Error::Status("AudioHardwareCreateProcessTap", status));
        }
        Ok(Self(id))
    }

    fn uid(&self) -> Option<String> {
        ca::string(self.0, ca::Address::new(TAP_UID, ca::SCOPE_GLOBAL))
    }

    fn is_float32(&self) -> bool {
        ca::get::<StreamDescription>(self.0, ca::Address::new(TAP_FORMAT, ca::SCOPE_GLOBAL))
            .is_some_and(|f| f.format == FORMAT_LPCM && f.flags & FLAG_FLOAT != 0 && f.bits == 32)
    }
}

impl Drop for Tap {
    fn drop(&mut self) {
        // SAFETY: el id es nuestro y ya no hay IO que lo lea (el agregado se
        // suelta antes).
        unsafe { AudioHardwareDestroyProcessTap(self.0) };
    }
}

struct Aggregate(ca::ObjectId);

impl Aggregate {
    /// Un agregado privado con los taps y, si hay, la salida real como
    /// principal.
    fn create(suffix: &str, output_uid: Option<&str>, taps: &[String]) -> Result<Self, Error> {
        let k = |s: &str| NSString::from_str(s);
        let yes = NSNumber::new_bool(true);
        // `kAudioSubTapUIDKey`, `kAudioSubTapDriftCompensationKey`.
        let tap_dicts: Vec<Retained<NSDictionary<NSString, AnyObject>>> = taps
            .iter()
            .map(|uid| {
                let uid = k(uid);
                NSDictionary::from_slices(&[&*k("uid"), &*k("drift")], &[obj(&uid), obj(&yes)])
            })
            .collect();
        let tap_list = NSArray::from_retained_slice(&tap_dicts);
        // `kAudioSubDeviceUIDKey`.
        let sub_dicts: Vec<Retained<NSDictionary<NSString, AnyObject>>> = output_uid
            .iter()
            .map(|uid| {
                let uid = k(uid);
                NSDictionary::from_slices(&[&*k("uid")], &[obj(&uid)])
            })
            .collect();
        let sub_list = NSArray::from_retained_slice(&sub_dicts);
        let uid = k(&format!("{AGGREGATE_PREFIX}{suffix}-{}", std::process::id()));
        let name = k("Atic (privado)");
        let main = k(output_uid.unwrap_or(""));
        // `kAudioAggregateDevice{UID,Name,IsPrivate,TapAutoStart,TapList,SubDeviceList,MainSubDevice}Key`.
        let mut keys = vec![k("uid"), k("name"), k("private"), k("tapautostart"), k("taps"), k("subdevices")];
        let mut values: Vec<&AnyObject> =
            vec![obj(&uid), obj(&name), obj(&yes), obj(&yes), obj(&tap_list), obj(&sub_list)];
        if output_uid.is_some() {
            keys.push(k("master"));
            values.push(obj(&main));
        }
        let key_refs: Vec<&NSString> = keys.iter().map(|k| &**k).collect();
        let dict = NSDictionary::from_slices(&key_refs, &values);
        let mut device = 0;
        // SAFETY: `NSDictionary` es puente gratuito con `CFDictionaryRef` y
        // vive durante la llamada.
        let status = unsafe { AudioHardwareCreateAggregateDevice(Retained::as_ptr(&dict).cast(), &mut device) };
        if status != 0 || device == 0 {
            return Err(Error::Status("AudioHardwareCreateAggregateDevice", status));
        }
        Ok(Self(device))
    }
}

impl Drop for Aggregate {
    fn drop(&mut self) {
        // SAFETY: id propio; Core Audio lo destruye por su cuenta.
        unsafe { AudioHardwareDestroyAggregateDevice(self.0) };
    }
}

/// Un IOProc andando sobre un dispositivo, con su dato de cliente.
struct Io<T> {
    device: ca::ObjectId,
    id: IoProcId,
    client: *mut T,
}

// SAFETY: el cliente es un `Arc` de atómicos; el IOProc corre en el hilo de
// audio de Core Audio y el dueño solo lo suelta en `Drop`.
unsafe impl<T: Send + Sync> Send for Io<T> {}

impl<T> Io<T> {
    fn start(device: ca::ObjectId, proc_: IoProc, client: Box<T>) -> Result<Self, Error> {
        let client = Box::into_raw(client);
        let mut id: IoProcId = std::ptr::null_mut();
        // SAFETY: `client` vive hasta que el IOProc se destruye (`Drop`).
        unsafe {
            let status = AudioDeviceCreateIOProcID(device, proc_, client.cast(), &mut id);
            if status != 0 {
                drop(Box::from_raw(client));
                return Err(Error::Status("AudioDeviceCreateIOProcID", status));
            }
            let status = AudioDeviceStart(device, id);
            if status != 0 {
                AudioDeviceDestroyIOProcID(device, id);
                drop(Box::from_raw(client));
                return Err(Error::Status("AudioDeviceStart", status));
            }
        }
        Ok(Self { device, id, client })
    }
}

impl<T> Drop for Io<T> {
    fn drop(&mut self) {
        // SAFETY: tras `Destroy` el HAL ya no llama al proc; recién ahí se
        // suelta el cliente.
        unsafe {
            AudioDeviceStop(self.device, self.id);
            AudioDeviceDestroyIOProcID(self.device, self.id);
            drop(Box::from_raw(self.client));
        }
    }
}

/// El pico (0..1) de todos los búferes float32 de una lista.
///
/// SAFETY: `list` es la lista que entrega Core Audio a un IOProc.
unsafe fn peak_of(list: &AudioBufferList) -> f32 {
    // La lista es de largo variable: `buffers` sigue en memoria `count` veces.
    let buffers = std::slice::from_raw_parts(list.buffers.as_ptr(), list.count as usize);
    let mut peak = 0f32;
    for buffer in buffers.iter().filter(|b| !b.data.is_null()) {
        let samples = std::slice::from_raw_parts(buffer.data as *const f32, buffer.bytes as usize / 4);
        for sample in samples {
            peak = peak.max(sample.abs());
        }
    }
    peak.min(1.0)
}

fn output_uid(device: Option<ca::ObjectId>) -> Option<String> {
    device.and_then(|d| ca::string(d, ca::Address::new(ca::UID, ca::SCOPE_GLOBAL)))
}

// --- Medidor ------------------------------------------------------------------

#[derive(Default)]
struct MeterShared {
    /// Pico desde la última lectura, como bits de f32 (para no negativos el
    /// orden de los bits es el de los números: sirve `fetch_max`).
    peak: AtomicU32,
    cycles: AtomicU64,
}

unsafe extern "C" fn meter_proc(
    _: ca::ObjectId,
    _: *const c_void,
    input: *const AudioBufferList,
    _: *const c_void,
    _: *mut AudioBufferList,
    _: *const c_void,
    client: *mut c_void,
) -> Status {
    let shared = &*(client as *const Arc<MeterShared>);
    if !input.is_null() {
        shared.peak.fetch_max(peak_of(&*input).to_bits(), Ordering::Relaxed);
    }
    shared.cycles.fetch_add(1, Ordering::Relaxed);
    0
}

/// Un tap global sin silenciar sobre un agregado privado: solo mide, no toca
/// lo que suena.
pub struct Meter {
    _io: Io<Arc<MeterShared>>,
    _aggregate: Aggregate,
    _tap: Tap,
    shared: Arc<MeterShared>,
    output: Option<ca::ObjectId>,
}

impl Meter {
    pub fn start() -> Result<Self, Error> {
        let desc = describe("Atic onda", &[], true, UNMUTED).ok_or(Error::Unsupported)?;
        let tap = Tap::create(&desc)?;
        if !tap.is_float32() {
            return Err(Error::Format);
        }
        let output = ca::default_device(false);
        let uid = tap.uid().ok_or(Error::Status("kAudioTapPropertyUID", -1))?;
        let aggregate = Aggregate::create("onda", output_uid(output).as_deref(), &[uid])?;
        let shared = Arc::new(MeterShared::default());
        let io = Io::start(aggregate.0, meter_proc, Box::new(shared.clone()))?;
        Ok(Self { _io: io, _aggregate: aggregate, _tap: tap, shared, output })
    }

    /// El pico desde la última lectura, y lo deja en cero.
    pub fn take_peak(&self) -> f32 {
        f32::from_bits(self.shared.peak.swap(0, Ordering::Relaxed))
    }

    /// Cuántos ciclos de IO corrieron: si no sube mientras algo suena, falta
    /// el permiso o el dispositivo se cayó.
    pub fn cycles(&self) -> u64 {
        self.shared.cycles.load(Ordering::Relaxed)
    }

    /// La salida por omisión ya no es la del agregado: hay que rehacerlo.
    pub fn stale(&self) -> bool {
        ca::default_device(false) != self.output
    }
}

// --- Volumen por app ----------------------------------------------------------

struct MixShared {
    gains: [AtomicU32; MAX_TAPS],
    cycles: AtomicU64,
}

unsafe extern "C" fn mix_proc(
    _: ca::ObjectId,
    _: *const c_void,
    input: *const AudioBufferList,
    _: *const c_void,
    output: *mut AudioBufferList,
    _: *const c_void,
    client: *mut c_void,
) -> Status {
    let shared = &*(client as *const Arc<MixShared>);
    shared.cycles.fetch_add(1, Ordering::Relaxed);
    if input.is_null() || output.is_null() {
        return 0;
    }
    let output = &mut *output;
    let outs = std::slice::from_raw_parts_mut(output.buffers.as_mut_ptr(), output.count as usize);
    let Some(out) = outs.first_mut().filter(|b| !b.data.is_null() && b.channels > 0) else {
        return 0;
    };
    let out_channels = out.channels as usize;
    let out_samples = std::slice::from_raw_parts_mut(out.data as *mut f32, out.bytes as usize / 4);
    let frames = out_samples.len() / out_channels;
    // El HAL entrega la salida en cero: se suma cada tap por su ganancia. Un
    // tap por búfer de entrada, en el orden de la lista `taps` del agregado.
    let ins = std::slice::from_raw_parts((*input).buffers.as_ptr(), (*input).count as usize);
    for (i, buffer) in ins.iter().enumerate().take(MAX_TAPS) {
        if buffer.data.is_null() || buffer.channels == 0 {
            continue;
        }
        let gain = f32::from_bits(shared.gains[i].load(Ordering::Relaxed));
        let in_channels = buffer.channels as usize;
        let samples = std::slice::from_raw_parts(buffer.data as *const f32, buffer.bytes as usize / 4);
        for frame in 0..frames.min(samples.len() / in_channels) {
            for channel in 0..out_channels {
                out_samples[frame * out_channels + channel] +=
                    samples[frame * in_channels + channel.min(in_channels - 1)] * gain;
            }
        }
    }
    0
}

struct Graph {
    _io: Io<Arc<MixShared>>,
    _aggregate: Aggregate,
    output: Option<ca::ObjectId>,
}

/// Volumen por app: un tap que silencia cada app con volumen bajo 100 % y un
/// agregado con la salida real que vuelve a emitir lo de esas apps por su
/// ganancia. A 100 % el tap se destruye y la app vuelve a sonar directo.
pub struct Mixer {
    /// pid, su tap y su ganancia. El orden es el de la lista `taps`.
    apps: Vec<(i32, Tap, f32)>,
    shared: Arc<MixShared>,
    graph: Option<Graph>,
}

impl Mixer {
    pub fn new() -> Self {
        let shared = Arc::new(MixShared {
            gains: std::array::from_fn(|_| AtomicU32::new(1f32.to_bits())),
            cycles: AtomicU64::new(0),
        });
        Self { apps: Vec::new(), shared, graph: None }
    }

    pub fn gain(&self, pid: i32) -> f32 {
        self.apps.iter().find(|(p, ..)| *p == pid).map_or(1.0, |(_, _, gain)| *gain)
    }

    pub fn set(&mut self, pid: i32, gain: f32) -> Result<(), Error> {
        let gain = gain.clamp(0.0, 1.0);
        let had = self.apps.iter().position(|(p, ..)| *p == pid);
        match (had, gain >= 0.999) {
            (Some(i), true) => {
                self.apps.remove(i);
                return self.rebuild();
            }
            (Some(i), false) => {
                self.apps[i].2 = gain;
                self.shared.gains[i].store(gain.to_bits(), Ordering::Relaxed);
                return Ok(());
            }
            (None, true) => return Ok(()),
            (None, false) => {}
        }
        if self.apps.len() >= MAX_TAPS {
            return Err(Error::Status("demasiadas apps con volumen propio", 0));
        }
        let object = process_object(pid).ok_or(Error::Status("TranslatePIDToProcessObject", 0))?;
        let desc = describe(&format!("Atic app {pid}"), &[object], false, MUTED_WHEN_TAPPED).ok_or(Error::Unsupported)?;
        let tap = Tap::create(&desc)?;
        self.apps.push((pid, tap, gain));
        self.rebuild()
    }

    /// Suelta las apps que ya no suenan y rehace el grafo si cambió la salida.
    pub fn tick(&mut self, alive: &[i32]) -> Result<(), Error> {
        let before = self.apps.len();
        self.apps.retain(|(pid, ..)| alive.contains(pid));
        let stale = self.graph.as_ref().is_some_and(|g| ca::default_device(false) != g.output);
        if before != self.apps.len() || stale {
            self.rebuild()
        } else {
            Ok(())
        }
    }

    /// Rehace el agregado con los taps vigentes (un corte breve). Suelta
    /// primero el IO y el agregado viejos; los taps quedan.
    fn rebuild(&mut self) -> Result<(), Error> {
        self.graph = None;
        if self.apps.is_empty() {
            return Ok(());
        }
        for (i, (_, _, gain)) in self.apps.iter().enumerate() {
            self.shared.gains[i].store(gain.to_bits(), Ordering::Relaxed);
        }
        let uids: Vec<String> = self.apps.iter().filter_map(|(_, tap, _)| tap.uid()).collect();
        let output = ca::default_device(false);
        let uid = output_uid(output).ok_or(Error::Status("sin salida por omisión", 0))?;
        let aggregate = Aggregate::create("apps", Some(&uid), &uids)?;
        let io = Io::start(aggregate.0, mix_proc, Box::new(self.shared.clone()))?;
        self.graph = Some(Graph { _io: io, _aggregate: aggregate, output });
        Ok(())
    }
}

impl Default for Mixer {
    fn default() -> Self {
        Self::new()
    }
}
