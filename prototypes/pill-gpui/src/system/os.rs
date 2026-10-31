//! Lo que Sistema le pide a Windows, en un hilo propio.
//!
//! Ahí vive COM (Core Audio y WMI) y la retención del café, que es por hilo:
//! tiene que ser uno que no termine. Los comandos llegan por un canal y se
//! juntan por tandas: arrastrar un slider manda decenas de valores y solo se
//! aplica el último de cada cosa.
//!
//! Como Atic (`system_control/`), salvo el brillo de la pantalla del
//! notebook, que aquí va por WMI (`WmiMonitorBrightnessMethods`): Atic solo
//! usa DDC/CI, que las pantallas internas casi nunca tienen.
//!
//! Rendimiento (temperaturas, GPU, red, discos) y las resoluciones de las
//! pantallas viven en `win.rs` y `mac.rs`; aquí se cablean al hilo.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc;

/// Una app que tiene sonido abierto. Una fila por proceso.
#[derive(Clone, Debug, PartialEq)]
pub struct AppAudio {
    pub pid: u32,
    pub name: String,
    pub path: Option<PathBuf>,
    pub volume: f32,
}

/// Una salida de audio (parlantes, audífonos, Bluetooth).
#[derive(Clone, Debug, PartialEq)]
pub struct Output {
    pub id: String,
    pub name: String,
    pub default: bool,
}

/// Cómo se le cambia el brillo a una pantalla.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DisplayId {
    /// La pantalla del notebook, por WMI.
    Internal,
    /// Un monitor por DDC/CI, por su `HMONITOR`.
    Monitor(isize),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Display {
    pub id: DisplayId,
    pub name: String,
    pub primary: bool,
    /// `None`: no deja cambiar el brillo.
    pub brightness: Option<f32>,
    /// Cómo la nombra el SO para cambiarle el modo: `\\.\DISPLAY1` en
    /// Windows, el `CGDirectDisplayID` en macOS.
    pub device: String,
    /// La resolución y frecuencia de ahora.
    pub mode: Option<Mode>,
    /// Las que ofrece el SO (sin repetir), para elegir otra.
    pub modes: Vec<Mode>,
}

/// Una resolución con su frecuencia.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Mode {
    /// Lo que se ve: en macOS, puntos (una Retina «parece» 1512 × 982); en
    /// Windows, píxeles.
    pub width: u32,
    pub height: u32,
    /// Hz redondeados; 0 si la pantalla no lo dice (algunas internas).
    pub hz: u32,
    /// macOS: el modo dibuja al doble (Retina). En Windows, siempre `false`.
    pub hidpi: bool,
    /// Con qué lo vuelve a encontrar el SO (`IODisplayModeID` en macOS).
    pub key: i64,
}

/// Un disco montado: el del sistema primero.
#[derive(Clone, Debug, PartialEq)]
pub struct Disk {
    pub name: String,
    pub free: u64,
    pub total: u64,
    /// Un pendrive o un disco externo.
    pub removable: bool,
}

/// Lo que dice la batería además del porcentaje.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Power {
    /// Capacidad de hoy sobre la de fábrica (0 – 1).
    pub health: Option<f32>,
    pub cycles: Option<u32>,
    /// Watts: positivos cargando, negativos gastando.
    pub watts: Option<f32>,
}

/// La pestaña Rendimiento: lo que se mide de todo el equipo.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Vitals {
    /// Uso de CPU de todo el equipo (0 – 100).
    pub cpu: f32,
    pub cpu_temp: Option<f32>,
    /// Uso de la GPU (0 – 100). `None` si el SO no lo dice.
    pub gpu: Option<f32>,
    pub gpu_temp: Option<f32>,
    pub ram_used: u64,
    pub ram_total: u64,
    /// Bytes por segundo, entre esta lectura y la anterior.
    pub net_down: f64,
    pub net_up: f64,
    pub disks: Vec<Disk>,
    /// macOS: 0 normal, 1 alto, 2 serio, 3 crítico (`thermalState`).
    pub thermal: Option<u8>,
    pub power: Option<Power>,
    /// Segundos desde que se encendió.
    pub uptime: u64,
}

/// Una app en la lista de procesos: sus procesos sumados.
#[derive(Clone, Debug, PartialEq)]
pub struct App {
    /// El ejecutable en minúsculas, sin `.exe`: la clave para cerrar.
    pub stem: String,
    pub name: String,
    pub path: Option<PathBuf>,
    pub cpu: f32,
    pub ram: u64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Procs {
    pub cpu: f32,
    pub ram_used: u64,
    pub ram_total: u64,
    pub apps: Vec<App>,
}

/// Lo que se ve en el panel.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Snapshot {
    pub volume: f32,
    pub muted: bool,
    /// Porcentaje y si está enchufado. `None` en un equipo sin batería.
    pub battery: Option<(u8, bool)>,
    pub awake: bool,
    pub displays: Vec<Display>,
    pub outputs: Vec<Output>,
    /// El micrófono por omisión está silenciado. `None` si no hay micrófono.
    pub mic_muted: Option<bool>,
    /// Solo si se pidió (la pestaña está abierta).
    pub audio: Vec<AppAudio>,
    pub procs: Option<Procs>,
    /// Pantallas conectadas, también las que duplican a otra (no se
    /// listan aparte).
    pub screens: usize,
    /// Alguna pantalla duplica a otra. Solo con Pantallas abierta.
    pub mirrored: bool,
    /// Modo oscuro del sistema. Solo con Pantallas abierta.
    pub dark: Option<bool>,
    pub vitals: Option<Vitals>,
}

/// Qué leer además de lo básico: enumerar procesos cuesta, y solo hace falta
/// con su pestaña abierta.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Want {
    pub audio: bool,
    pub procs: bool,
    /// Resoluciones, duplicar y modo oscuro.
    pub displays: bool,
    pub vitals: bool,
}

pub enum Cmd {
    Volume(f32),
    Mute(bool),
    AppVolume(u32, f32),
    Brightness(DisplayId, f32),
    Awake(bool),
    MicMute(bool),
    /// Cambiar la salida de audio por omisión.
    Output(String),
    /// Pedir que cierre (las ventanas reciben `WM_CLOSE`, como la ×).
    Close(String),
    /// Terminar los procesos: lo que no se cierra por las buenas.
    Force(String),
    /// Cambiar la resolución (y frecuencia) de una pantalla.
    Mode(DisplayId, Mode),
    /// Que esta sea la pantalla principal (la de la barra de menús o de tareas).
    Primary(DisplayId),
    /// Duplicar las pantallas (`true`) o extenderlas.
    Mirror(bool),
    /// Modo oscuro del sistema.
    Dark(bool),
    /// Abrir donde el SO conecta una pantalla inalámbrica (AirPlay, Miracast).
    ConnectDisplay,
    Read(Want, futures::channel::oneshot::Sender<Snapshot>),
}

/// El resultado de cerrar, para avisar en el panel.
pub type Outcome = Result<String, String>;

#[derive(Clone)]
pub struct Backend {
    tx: mpsc::Sender<Cmd>,
    pub outcomes: std::sync::Arc<std::sync::Mutex<Vec<Outcome>>>,
}

impl Backend {
    pub fn start() -> Self {
        let (tx, rx) = mpsc::channel::<Cmd>();
        let outcomes = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = outcomes.clone();
        std::thread::Builder::new()
            .name("sistema".into())
            .spawn(move || imp::run(rx, sink))
            .expect("hilo del sistema");
        Self { tx, outcomes }
    }

    pub fn send(&self, cmd: Cmd) {
        let _ = self.tx.send(cmd);
    }

    pub fn read(&self, want: Want) -> futures::channel::oneshot::Receiver<Snapshot> {
        let (tx, rx) = futures::channel::oneshot::channel();
        self.send(Cmd::Read(want, tx));
        rx
    }
}

/// De una tanda de comandos, el último valor de cada cosa.
#[derive(Default, Debug, PartialEq)]
pub struct Batch {
    pub volume: Option<f32>,
    pub mute: Option<bool>,
    pub apps: HashMap<u32, f32>,
    pub brightness: HashMap<DisplayId, f32>,
    pub awake: Option<bool>,
    pub mic_mute: Option<bool>,
    pub output: Option<String>,
    pub close: Vec<String>,
    pub force: Vec<String>,
    pub modes: HashMap<DisplayId, Mode>,
    pub primary: Option<DisplayId>,
    pub mirror: Option<bool>,
    pub dark: Option<bool>,
    pub connect: bool,
    pub want: Option<Want>,
}

pub fn batch(cmds: &[&Cmd]) -> Batch {
    let mut out = Batch::default();
    for cmd in cmds {
        match cmd {
            Cmd::Volume(v) => out.volume = Some(*v),
            Cmd::Mute(m) => out.mute = Some(*m),
            Cmd::AppVolume(pid, v) => {
                out.apps.insert(*pid, *v);
            }
            Cmd::Brightness(id, b) => {
                out.brightness.insert(*id, *b);
            }
            Cmd::Awake(a) => out.awake = Some(*a),
            Cmd::MicMute(m) => out.mic_mute = Some(*m),
            Cmd::Output(id) => out.output = Some(id.clone()),
            Cmd::Close(stem) => out.close.push(stem.clone()),
            Cmd::Force(stem) => out.force.push(stem.clone()),
            Cmd::Mode(id, mode) => {
                out.modes.insert(*id, mode.clone());
            }
            Cmd::Primary(id) => out.primary = Some(*id),
            Cmd::Mirror(on) => out.mirror = Some(*on),
            Cmd::Dark(on) => out.dark = Some(*on),
            Cmd::ConnectDisplay => out.connect = true,
            Cmd::Read(want, _) => {
                let prev = out.want.unwrap_or_default();
                out.want = Some(Want {
                    audio: prev.audio || want.audio,
                    procs: prev.procs || want.procs,
                    displays: prev.displays || want.displays,
                    vitals: prev.vitals || want.vitals,
                });
            }
        }
    }
    out
}

/// Bytes por segundo entre dos lecturas de los contadores de red. La
/// primera lectura (o un contador que volvió atrás, por un adaptador que se
/// fue) da cero en vez de un pico.
pub fn net_rate(prev: &mut Option<(u64, u64, std::time::Instant)>, down: u64, up: u64) -> (f64, f64) {
    let now = std::time::Instant::now();
    let rate = match prev.as_ref() {
        Some(&(d0, u0, at)) if down >= d0 && up >= u0 => {
            let secs = now.saturating_duration_since(at).as_secs_f64();
            if secs > 0.05 {
                ((down - d0) as f64 / secs, (up - u0) as f64 / secs)
            } else {
                (0.0, 0.0)
            }
        }
        _ => (0.0, 0.0),
    };
    *prev = Some((down, up, now));
    rate
}

/// Modo oscuro: solo se toca si no está ya como se pide (el atajo del
/// lanzador lo alterna).
pub fn set_dark(dark: bool) -> Result<(), String> {
    if crate::theme::windows_light() != dark {
        return Ok(());
    }
    crate::launcher::system_action(crate::launcher::Action::DarkMode)
}

/// Un error al cambiar algo de las pantallas, como aviso en el panel.
pub fn report(outcomes: &std::sync::Arc<std::sync::Mutex<Vec<Outcome>>>, result: Result<(), String>) {
    if let Err(error) = result {
        eprintln!("sistema: {error}");
        outcomes.lock().map(|mut o| o.push(Err(error))).ok();
    }
}

/// Procesos que no se ofrecen: el SO y este mismo prototipo. Cerrarlos no es
/// una función, es un susto.
pub fn hidden_process(stem: &str) -> bool {
    matches!(
        stem,
        "system"
            | "idle"
            | "registry"
            | "memory compression"
            | "secure system"
            | "smss"
            | "csrss"
            | "wininit"
            | "winlogon"
            | "services"
            | "lsass"
            | "lsaiso"
            | "svchost"
            | "dwm"
            | "fontdrvhost"
            | "explorer"
            | "sihost"
            | "ctfmon"
            | "conhost"
            | "audiodg"
            | "runtimebroker"
            | "searchhost"
            | "startmenuexperiencehost"
            | "shellexperiencehost"
            | "textinputhost"
            | "msmpeng"
            | "nissrv"
            | "securityhealthservice"
            | "spoolsv"
            | "wudfhost"
            | "dllhost"
            | "taskhostw"
            | "pill-gpui"
            | "atic"
            | "atic-desktop"
    )
}

/// Agrupa por ejecutable y ordena por CPU y después por memoria.
pub fn group(rows: Vec<(String, u32, Option<PathBuf>, f32, u64)>, limit: usize) -> Vec<App> {
    let mut map: HashMap<String, App> = HashMap::new();
    for (stem, _pid, path, cpu, ram) in rows {
        if stem.is_empty() || hidden_process(&stem) {
            continue;
        }
        let entry = map.entry(stem.clone()).or_insert_with(|| App {
            name: pretty(&stem),
            stem,
            path: None,
            cpu: 0.0,
            ram: 0,
        });
        entry.cpu += cpu;
        entry.ram = entry.ram.saturating_add(ram);
        if entry.path.is_none() {
            entry.path = path;
        }
    }
    let mut apps: Vec<App> = map.into_values().collect();
    apps.sort_by(|a, b| b.cpu.total_cmp(&a.cpu).then(b.ram.cmp(&a.ram)));
    apps.truncate(limit);
    for app in &mut apps {
        app.cpu = app.cpu.clamp(0.0, 100.0);
    }
    apps
}

/// `msedgewebview2` → `Msedgewebview2`, mientras no haya un nombre mejor.
pub fn pretty(stem: &str) -> String {
    let mut chars = stem.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

#[cfg(windows)]
mod imp {
    use super::*;
    use std::time::{Duration, Instant};
    use windows::core::{w, Interface, BSTR, PCWSTR, PWSTR};
    use windows::Win32::Devices::Display::{
        DestroyPhysicalMonitors, GetMonitorBrightness, GetNumberOfPhysicalMonitorsFromHMONITOR,
        GetPhysicalMonitorsFromHMONITOR, SetMonitorBrightness, PHYSICAL_MONITOR,
    };
    use windows::core::BOOL;
    use windows::Win32::Foundation::{CloseHandle, FILETIME, HWND, LPARAM, RECT, WPARAM};
    use windows::Win32::Graphics::Gdi::{
        EnumDisplayDevicesW, EnumDisplayMonitors, GetMonitorInfoW, DISPLAY_DEVICEW, HDC, HMONITOR,
        MONITORINFO, MONITORINFOEXW,
    };
    use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
    use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
    use windows::Win32::Media::Audio::{
        eCapture, eCommunications, eConsole, eMultimedia, eRender, EDataFlow,
        IAudioSessionControl, IAudioSessionControl2, IAudioSessionManager2, IMMDevice,
        IMMDeviceEnumerator, ISimpleAudioVolume, MMDeviceEnumerator, DEVICE_STATE_ACTIVE,
    };
    use windows::Win32::System::Com::{CoTaskMemFree, STGM_READ};
    use windows::Win32::Storage::FileSystem::{
        GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW,
    };
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoSetProxyBlanket, CLSCTX_ALL, CLSCTX_INPROC_SERVER,
        COINIT_MULTITHREADED, EOAC_NONE, RPC_C_AUTHN_LEVEL_CALL, RPC_C_IMP_LEVEL_IMPERSONATE,
    };
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    use windows::Win32::System::Power::{
        GetSystemPowerStatus, SetThreadExecutionState, ES_CONTINUOUS, ES_DISPLAY_REQUIRED,
        ES_SYSTEM_REQUIRED, SYSTEM_POWER_STATUS,
    };
    use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
    use windows::Win32::System::Rpc::{RPC_C_AUTHN_WINNT, RPC_C_AUTHZ_NONE};
    use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    use windows::Win32::System::Threading::{
        GetProcessTimes, GetSystemTimes, OpenProcess, QueryFullProcessImageNameW,
        TerminateProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
        PROCESS_TERMINATE,
    };
    use windows::Win32::System::Variant::VARIANT;
    use windows::Win32::System::Wmi::{
        IWbemClassObject, IWbemLocator, IWbemServices, WbemLocator, WBEM_FLAG_FORWARD_ONLY,
        WBEM_FLAG_RETURN_IMMEDIATELY, WBEM_GENERIC_FLAG_TYPE, WBEM_INFINITE,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowThreadProcessId, IsWindowVisible, PostMessageW, WM_CLOSE,
    };

    struct State {
        audio: Option<IAudioEndpointVolume>,
        wmi: Option<IWbemServices>,
        awake: bool,
        /// Para la CPU hacen falta dos muestras: la anterior.
        cpu_sample: Option<(u64, u64, HashMap<u32, u64>)>,
        /// Nombre del producto por ejecutable: leerlo abre el archivo.
        names: HashMap<PathBuf, String>,
        displays: Vec<Display>,
        displays_at: Option<Instant>,
        /// Las muestras de Rendimiento (CPU, red, GPU) entre lecturas.
        vitals: crate::system::win::Sampler,
    }

    pub fn run(rx: mpsc::Receiver<Cmd>, outcomes: std::sync::Arc<std::sync::Mutex<Vec<Outcome>>>) {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        let mut state = State {
            audio: endpoint(),
            wmi: wmi(),
            awake: false,
            cpu_sample: None,
            names: HashMap::new(),
            displays: Vec::new(),
            displays_at: None,
            vitals: Default::default(),
        };
        while let Ok(first) = rx.recv() {
            let mut cmds = vec![first];
            while let Ok(more) = rx.try_recv() {
                cmds.push(more);
            }
            let b = batch(&cmds.iter().collect::<Vec<_>>());
            if let Some(m) = b.mute {
                with_audio(&mut state, |v| unsafe { v.SetMute(m, std::ptr::null()) });
            }
            if let Some(level) = b.volume {
                with_audio(&mut state, |v| unsafe {
                    v.SetMasterVolumeLevelScalar(level.clamp(0.0, 1.0), std::ptr::null())
                });
            }
            if let Some(id) = b.output.as_ref() {
                if let Err(error) = set_default_output(id) {
                    eprintln!("sistema: salida de audio: {error}");
                }
                // El volumen general ahora es el de la salida nueva.
                state.audio = endpoint();
            }
            if let Some(m) = b.mic_mute {
                if let Some(mic) = device_volume(eCapture) {
                    unsafe {
                        let _ = mic.SetMute(m, std::ptr::null());
                    }
                }
            }
            if !b.apps.is_empty() {
                set_app_volumes(&b.apps);
            }
            for (id, level) in &b.brightness {
                let result = match id {
                    DisplayId::Internal => set_wmi_brightness(&state, *level),
                    DisplayId::Monitor(handle) => set_ddc_brightness(*handle, *level),
                };
                if let Err(error) = result {
                    eprintln!("sistema: brillo: {error}");
                }
                if let Some(display) = state.displays.iter_mut().find(|d| d.id == *id) {
                    display.brightness = Some(level.clamp(0.0, 1.0));
                }
            }
            if let Some(on) = b.awake {
                let flags = if on {
                    ES_CONTINUOUS | ES_SYSTEM_REQUIRED | ES_DISPLAY_REQUIRED
                } else {
                    ES_CONTINUOUS
                };
                if unsafe { SetThreadExecutionState(flags) }.0 != 0 {
                    state.awake = on;
                }
            }
            for stem in &b.close {
                let closed = close(stem);
                let outcome = if closed > 0 {
                    Ok(crate::i18n::tf("pill.system.closeRequested", &[("name", &pretty(stem))]))
                } else {
                    Err(crate::i18n::tf("pill.system.noWindows", &[("name", &pretty(stem))]))
                };
                outcomes.lock().map(|mut o| o.push(outcome)).ok();
            }
            for stem in &b.force {
                let outcome = force(stem)
                    .map(|n| crate::i18n::tf("pill.system.closedProcs", &[("name", &pretty(stem)), ("n", &n)]));
                outcomes.lock().map(|mut o| o.push(outcome)).ok();
            }
            let device_of = |state: &State, id: DisplayId| {
                state.displays.iter().find(|d| d.id == id).map(|d| d.device.clone())
            };
            for (id, mode) in &b.modes {
                if let Some(device) = device_of(&state, *id) {
                    report(&outcomes, crate::system::win::set_mode(&device, mode));
                }
            }
            if let Some(id) = b.primary {
                if let Some(device) = device_of(&state, id) {
                    report(&outcomes, crate::system::win::set_primary(&device));
                }
            }
            if let Some(on) = b.mirror {
                report(&outcomes, crate::system::win::set_mirror(on));
            }
            if !b.modes.is_empty() || b.primary.is_some() || b.mirror.is_some() {
                // Lo que se cambió se ve en la próxima lectura, sin esperar 10 s.
                state.displays_at = None;
            }
            if let Some(dark) = b.dark {
                report(&outcomes, set_dark(dark));
            }
            if b.connect {
                report(&outcomes, crate::system::win::connect());
            }
            if let Some(want) = b.want {
                let snapshot = read(&mut state, want);
                for cmd in cmds {
                    if let Cmd::Read(_, reply) = cmd {
                        let _ = reply.send(snapshot.clone());
                    }
                }
            }
        }
    }

    // --- Lectura -------------------------------------------------------------

    fn read(state: &mut State, want: Want) -> Snapshot {
        // La salida por omisión puede haber cambiado (audífonos).
        state.audio = endpoint().or(state.audio.take());
        let (volume, muted) = state
            .audio
            .as_ref()
            .and_then(|v| unsafe {
                Some((v.GetMasterVolumeLevelScalar().ok()?, v.GetMute().ok()?.as_bool()))
            })
            .unwrap_or((0.0, false));
        // Las pantallas no cambian a cada rato y preguntarle a DDC/CI tarda:
        // se reenumeran cada 10 s; el brillo interno se relee siempre.
        let stale = state
            .displays_at
            .is_none_or(|at| at.elapsed() > Duration::from_secs(10));
        if stale {
            state.displays = displays(state);
            state.displays_at = Some(Instant::now());
        } else if let Some(level) = wmi_brightness(state) {
            for display in &mut state.displays {
                if display.id == DisplayId::Internal {
                    display.brightness = Some(level);
                }
            }
        }
        let audio = if want.audio { app_audio(state) } else { Vec::new() };
        let procs = want.procs.then(|| procs(state));
        let mic_muted = device_volume(eCapture)
            .and_then(|mic| unsafe { mic.GetMute().ok() })
            .map(|m| m.as_bool());
        let vitals = want.vitals.then(|| crate::system::win::vitals(&mut state.vitals, state.wmi.as_ref()));
        Snapshot {
            volume,
            muted,
            battery: battery(),
            awake: state.awake,
            displays: state.displays.clone(),
            outputs: outputs(),
            mic_muted,
            audio,
            procs,
            screens: crate::system::win::screens().max(state.displays.len()),
            mirrored: want.displays && crate::system::win::mirrored(),
            dark: want.displays.then(|| !crate::theme::windows_light()),
            vitals,
        }
    }

    fn battery() -> Option<(u8, bool)> {
        let mut status = SYSTEM_POWER_STATUS::default();
        unsafe { GetSystemPowerStatus(&mut status).ok()? };
        // 128: sin batería; más de 100: desconocido.
        if status.BatteryFlag & 128 != 0 || status.BatteryLifePercent > 100 {
            return None;
        }
        Some((status.BatteryLifePercent, status.ACLineStatus == 1))
    }

    // --- Audio -----------------------------------------------------------------

    fn endpoint() -> Option<IAudioEndpointVolume> {
        unsafe {
            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
            let device = enumerator.GetDefaultAudioEndpoint(eRender, eConsole).ok()?;
            device.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None).ok()
        }
    }

    fn enumerator() -> Option<IMMDeviceEnumerator> {
        unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok() }
    }

    /// El volumen del dispositivo por omisión de entrada o de salida.
    fn device_volume(flow: EDataFlow) -> Option<IAudioEndpointVolume> {
        unsafe {
            let device = enumerator()?.GetDefaultAudioEndpoint(flow, eConsole).ok()?;
            device.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None).ok()
        }
    }

    fn device_id(device: &IMMDevice) -> Option<String> {
        unsafe {
            let raw = device.GetId().ok()?;
            let id = raw.to_string().ok();
            CoTaskMemFree(Some(raw.0 as *const _));
            id
        }
    }

    /// Las salidas activas, con la de omisión marcada.
    fn outputs() -> Vec<Output> {
        let Some(enumerator) = enumerator() else {
            return Vec::new();
        };
        unsafe {
            let current = enumerator
                .GetDefaultAudioEndpoint(eRender, eConsole)
                .ok()
                .and_then(|d| device_id(&d));
            let Ok(list) = enumerator.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE) else {
                return Vec::new();
            };
            let count = list.GetCount().unwrap_or(0);
            (0..count)
                .filter_map(|i| {
                    let device = list.Item(i).ok()?;
                    let id = device_id(&device)?;
                    let name = device
                        .OpenPropertyStore(STGM_READ)
                        .ok()
                        .and_then(|store| store.GetValue(&PKEY_Device_FriendlyName).ok())
                        .and_then(|value| BSTR::try_from(&value).ok())
                        .map(|name| name.to_string())
                        .unwrap_or_else(|| crate::i18n::t("pill.system.audioOutput").into());
                    Some(Output {
                        default: current.as_deref() == Some(id.as_str()),
                        id,
                        name,
                    })
                })
                .collect()
        }
    }

    /// Cambia la salida por omisión con `IPolicyConfig`, la interfaz que usa
    /// el propio panel de sonido de Windows. No está documentada, pero es
    /// estable desde Windows 7 y la usan todos los cambiadores de salida.
    fn set_default_output(id: &str) -> windows::core::Result<()> {
        const CLSID_POLICY_CONFIG: windows::core::GUID =
            windows::core::GUID::from_u128(0x870af99c_171d_4f9e_af0d_e63df40c2bc9);
        const IID_POLICY_CONFIG: windows::core::GUID =
            windows::core::GUID::from_u128(0xf8679f50_850a_41cf_9c72_430f290290c8);
        /// `SetDefaultEndpoint` es la 14.ª entrada de la tabla (contando las
        /// tres de `IUnknown`).
        const SET_DEFAULT_ENDPOINT: usize = 13;
        type SetDefault = unsafe extern "system" fn(
            *mut std::ffi::c_void,
            PCWSTR,
            u32,
        ) -> windows::core::HRESULT;
        unsafe {
            let unknown: windows::core::IUnknown =
                CoCreateInstance(&CLSID_POLICY_CONFIG, None, CLSCTX_ALL)?;
            let mut policy = std::ptr::null_mut();
            unknown.query(&IID_POLICY_CONFIG, &mut policy).ok()?;
            let vtable = *(policy as *const *const usize);
            let set: SetDefault = std::mem::transmute(*vtable.add(SET_DEFAULT_ENDPOINT));
            let wide: Vec<u16> = id.encode_utf16().chain(Some(0)).collect();
            let mut result = Ok(());
            for role in [eConsole, eMultimedia, eCommunications] {
                let hr = set(policy, PCWSTR(wide.as_ptr()), role.0 as u32);
                if hr.is_err() {
                    result = Err(hr.into());
                }
            }
            // Soltar la referencia que dio `query`.
            let release: unsafe extern "system" fn(*mut std::ffi::c_void) -> u32 =
                std::mem::transmute(*vtable.add(2));
            release(policy);
            result
        }
    }

    fn with_audio(state: &mut State, f: impl Fn(&IAudioEndpointVolume) -> windows::core::Result<()>) {
        let ok = state.audio.as_ref().is_some_and(|v| f(v).is_ok());
        if !ok {
            state.audio = endpoint();
            if let Some(v) = state.audio.as_ref() {
                let _ = f(v);
            }
        }
    }

    /// Cada sesión de audio de la salida actual, con su proceso.
    fn sessions() -> Vec<(u32, IAudioSessionControl)> {
        unsafe {
            let Ok(enumerator) =
                CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL)
            else {
                return Vec::new();
            };
            let Ok(device) = enumerator.GetDefaultAudioEndpoint(eRender, eConsole) else {
                return Vec::new();
            };
            let Ok(manager) = device.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None) else {
                return Vec::new();
            };
            let Ok(list) = manager.GetSessionEnumerator() else {
                return Vec::new();
            };
            let count = list.GetCount().unwrap_or(0);
            (0..count)
                .filter_map(|i| {
                    let control = list.GetSession(i).ok()?;
                    let control2: IAudioSessionControl2 = control.cast().ok()?;
                    let pid = control2.GetProcessId().ok()?;
                    (pid != 0).then_some((pid, control))
                })
                .collect()
        }
    }

    fn app_audio(state: &mut State) -> Vec<AppAudio> {
        let mut out: Vec<AppAudio> = Vec::new();
        for (pid, control) in sessions() {
            // Una fila por app: un navegador trae varias sesiones.
            if out.iter().any(|a| a.pid == pid) {
                continue;
            }
            let Ok(simple) = control.cast::<ISimpleAudioVolume>() else {
                continue;
            };
            let volume = unsafe { simple.GetMasterVolume() }.unwrap_or(0.0);
            let path = process_path(pid);
            let name = path
                .as_ref()
                .map(|p| product_name(state, p))
                .unwrap_or_else(|| crate::i18n::tf("pill.system.appPid", &[("pid", &pid)]));
            out.push(AppAudio {
                pid,
                name,
                path,
                volume: volume.clamp(0.0, 1.0),
            });
        }
        out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        out
    }

    /// A todas las sesiones de cada proceso: con solo la primera, mover el
    /// control de un navegador no cambiaba lo que se oía.
    fn set_app_volumes(levels: &HashMap<u32, f32>) {
        for (pid, control) in sessions() {
            let Some(level) = levels.get(&pid) else {
                continue;
            };
            if let Ok(simple) = control.cast::<ISimpleAudioVolume>() {
                unsafe {
                    let _ = simple.SetMasterVolume(level.clamp(0.0, 1.0), std::ptr::null());
                }
            }
        }
    }

    // --- Pantallas -------------------------------------------------------------

    /// Monitor, nombre, si es la principal y su dispositivo (`\\.\DISPLAY1`).
    struct Monitors(Vec<(HMONITOR, String, bool, String)>);

    unsafe extern "system" fn each_monitor(
        monitor: HMONITOR,
        _: HDC,
        _: *mut RECT,
        data: LPARAM,
    ) -> BOOL {
        let list = &mut *(data.0 as *mut Monitors);
        let mut info = MONITORINFOEXW::default();
        info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
        if GetMonitorInfoW(monitor, &mut info as *mut MONITORINFOEXW as *mut MONITORINFO).as_bool() {
            let mut device = DISPLAY_DEVICEW {
                cb: std::mem::size_of::<DISPLAY_DEVICEW>() as u32,
                ..Default::default()
            };
            let name = if EnumDisplayDevicesW(PCWSTR(info.szDevice.as_ptr()), 0, &mut device, 0)
                .as_bool()
            {
                wide(&device.DeviceString)
            } else {
                String::new()
            };
            list.0.push((monitor, name, info.monitorInfo.dwFlags & 1 != 0, wide(&info.szDevice)));
        }
        BOOL(1)
    }

    /// Las pantallas conectadas. La que no responde a DDC/CI y tiene brillo
    /// por WMI es la del notebook.
    fn displays(state: &State) -> Vec<Display> {
        let mut monitors = Monitors(Vec::new());
        unsafe {
            let _ = EnumDisplayMonitors(
                None,
                None,
                Some(each_monitor),
                LPARAM(&mut monitors as *mut Monitors as isize),
            );
        }
        let internal = wmi_brightness(state);
        let mut internal_used = false;
        let mut out = Vec::new();
        for (index, (monitor, name, primary, device)) in monitors.0.into_iter().enumerate() {
            let ddc = ddc_brightness(monitor);
            let (id, brightness, label) = if let Some(level) = ddc {
                (DisplayId::Monitor(monitor.0 as isize), Some(level), monitor_name(&name, index))
            } else if internal.is_some() && !internal_used {
                internal_used = true;
                (DisplayId::Internal, internal, crate::i18n::t("pill.system.thisDisplay").to_string())
            } else {
                (DisplayId::Monitor(monitor.0 as isize), None, monitor_name(&name, index))
            };
            let (mode, modes) = crate::system::win::modes(&device);
            out.push(Display {
                id,
                name: label,
                primary,
                brightness,
                device,
                mode,
                modes,
            });
        }
        // La que tiene control primero: es la del slider grande.
        out.sort_by_key(|d| (d.brightness.is_none(), !d.primary));
        out
    }

    fn monitor_name(name: &str, index: usize) -> String {
        if name.is_empty() || name.to_lowercase().contains("generic") || name.contains("PnP") {
            crate::i18n::tf("pill.system.monitorN", &[("n", &(index + 1))])
        } else {
            name.to_string()
        }
    }

    fn physical(monitor: HMONITOR) -> Option<Vec<PHYSICAL_MONITOR>> {
        unsafe {
            let mut count = 0u32;
            GetNumberOfPhysicalMonitorsFromHMONITOR(monitor, &mut count).ok()?;
            if count == 0 {
                return None;
            }
            let mut list = vec![PHYSICAL_MONITOR::default(); count as usize];
            GetPhysicalMonitorsFromHMONITOR(monitor, &mut list).ok()?;
            Some(list)
        }
    }

    fn ddc_brightness(monitor: HMONITOR) -> Option<f32> {
        let mut list = physical(monitor)?;
        let (mut min, mut cur, mut max) = (0u32, 0u32, 0u32);
        let ok = unsafe { GetMonitorBrightness(list[0].hPhysicalMonitor, &mut min, &mut cur, &mut max) };
        unsafe {
            let _ = DestroyPhysicalMonitors(&mut list);
        }
        (ok != 0 && max > min).then(|| (cur.saturating_sub(min)) as f32 / (max - min) as f32)
    }

    fn set_ddc_brightness(handle: isize, level: f32) -> windows::core::Result<()> {
        let monitor = HMONITOR(handle as *mut _);
        let Some(mut list) = physical(monitor) else {
            return Ok(());
        };
        unsafe {
            let (mut min, mut cur, mut max) = (0u32, 0u32, 0u32);
            if GetMonitorBrightness(list[0].hPhysicalMonitor, &mut min, &mut cur, &mut max) != 0 {
                let value = min + ((max - min) as f32 * level.clamp(0.0, 1.0)).round() as u32;
                SetMonitorBrightness(list[0].hPhysicalMonitor, value.clamp(min, max));
            }
            let _ = DestroyPhysicalMonitors(&mut list);
        }
        Ok(())
    }

    fn wmi() -> Option<IWbemServices> {
        unsafe {
            let locator: IWbemLocator =
                CoCreateInstance(&WbemLocator, None, CLSCTX_INPROC_SERVER).ok()?;
            let services = locator
                .ConnectServer(
                    &BSTR::from("ROOT\\WMI"),
                    &BSTR::new(),
                    &BSTR::new(),
                    &BSTR::new(),
                    0,
                    &BSTR::new(),
                    None,
                )
                .ok()?;
            CoSetProxyBlanket(
                &services,
                RPC_C_AUTHN_WINNT,
                RPC_C_AUTHZ_NONE,
                None,
                RPC_C_AUTHN_LEVEL_CALL,
                RPC_C_IMP_LEVEL_IMPERSONATE,
                None,
                EOAC_NONE,
            )
            .ok()?;
            Some(services)
        }
    }

    fn first(services: &IWbemServices, query: &str) -> Option<IWbemClassObject> {
        unsafe {
            let flags =
                WBEM_GENERIC_FLAG_TYPE(WBEM_FLAG_FORWARD_ONLY.0 | WBEM_FLAG_RETURN_IMMEDIATELY.0);
            let rows = services
                .ExecQuery(&BSTR::from("WQL"), &BSTR::from(query), flags, None)
                .ok()?;
            let mut row = [None];
            let mut got = 0u32;
            let _ = rows.Next(WBEM_INFINITE, &mut row, &mut got);
            if got == 0 {
                return None;
            }
            row[0].take()
        }
    }

    fn wmi_brightness(state: &State) -> Option<f32> {
        let services = state.wmi.as_ref()?;
        let object = first(services, "SELECT CurrentBrightness FROM WmiMonitorBrightness")?;
        let mut value = VARIANT::default();
        unsafe {
            object.Get(w!("CurrentBrightness"), 0, &mut value, None, None).ok()?;
        }
        let level = u32::try_from(&value).ok()?;
        Some(level.min(100) as f32 / 100.0)
    }

    /// `WmiSetBrightness(Timeout, Brightness)` sobre la pantalla interna.
    fn set_wmi_brightness(state: &State, level: f32) -> windows::core::Result<()> {
        let Some(services) = state.wmi.as_ref() else {
            return Ok(());
        };
        let Some(instance) = first(services, "SELECT * FROM WmiMonitorBrightnessMethods") else {
            return Ok(());
        };
        unsafe {
            let mut path = VARIANT::default();
            instance.Get(w!("__PATH"), 0, &mut path, None, None)?;
            let path = BSTR::try_from(&path)?;
            let mut class = None;
            services.GetObject(
                &BSTR::from("WmiMonitorBrightnessMethods"),
                WBEM_GENERIC_FLAG_TYPE(0),
                None,
                Some(&mut class),
                None,
            )?;
            let Some(class) = class else {
                return Ok(());
            };
            let mut signature = None;
            class.GetMethod(w!("WmiSetBrightness"), 0, &mut signature, std::ptr::null_mut())?;
            let Some(signature) = signature else {
                return Ok(());
            };
            let params = signature.SpawnInstance(0)?;
            params.Put(w!("Timeout"), 0, &VARIANT::from(0i32), 0)?;
            let percent = (level.clamp(0.0, 1.0) * 100.0).round() as i32;
            params.Put(w!("Brightness"), 0, &VARIANT::from(percent), 0)?;
            services.ExecMethod(
                &path,
                &BSTR::from("WmiSetBrightness"),
                WBEM_GENERIC_FLAG_TYPE(0),
                None,
                &params,
                None,
                None,
            )
        }
    }

    // --- Procesos ----------------------------------------------------------------

    fn filetime(t: FILETIME) -> u64 {
        (t.dwHighDateTime as u64) << 32 | t.dwLowDateTime as u64
    }

    fn wide(buf: &[u16]) -> String {
        let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        String::from_utf16_lossy(&buf[..end])
    }

    fn process_path(pid: u32) -> Option<PathBuf> {
        unsafe {
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
            let mut buffer = [0u16; 1024];
            let mut len = buffer.len() as u32;
            let ok = QueryFullProcessImageNameW(
                handle,
                PROCESS_NAME_WIN32,
                PWSTR(buffer.as_mut_ptr()),
                &mut len,
            );
            let _ = CloseHandle(handle);
            ok.ok()?;
            Some(PathBuf::from(String::from_utf16_lossy(&buffer[..len as usize])))
        }
    }

    /// La descripción del ejecutable («Google Chrome»), o su nombre.
    fn product_name(state: &mut State, path: &PathBuf) -> String {
        if let Some(name) = state.names.get(path) {
            return name.clone();
        }
        let name = file_description(path).unwrap_or_else(|| {
            pretty(
                &path
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_lowercase())
                    .unwrap_or_default(),
            )
        });
        state.names.insert(path.clone(), name.clone());
        name
    }

    fn file_description(path: &PathBuf) -> Option<String> {
        let wide_path: Vec<u16> = path.as_os_str().encode_wide_null();
        unsafe {
            let size = GetFileVersionInfoSizeW(PCWSTR(wide_path.as_ptr()), None);
            if size == 0 {
                return None;
            }
            let mut data = vec![0u8; size as usize];
            GetFileVersionInfoW(PCWSTR(wide_path.as_ptr()), None, size, data.as_mut_ptr() as *mut _)
                .ok()?;
            let mut ptr = std::ptr::null_mut();
            let mut len = 0u32;
            if !VerQueryValueW(
                data.as_ptr() as *const _,
                w!("\\VarFileInfo\\Translation"),
                &mut ptr,
                &mut len,
            )
            .as_bool()
                || len < 4
            {
                return None;
            }
            let lang = *(ptr as *const u16);
            let page = *(ptr as *const u16).add(1);
            let key: Vec<u16> =
                format!("\\StringFileInfo\\{lang:04x}{page:04x}\\FileDescription")
                    .encode_utf16()
                    .chain(Some(0))
                    .collect();
            if !VerQueryValueW(data.as_ptr() as *const _, PCWSTR(key.as_ptr()), &mut ptr, &mut len)
                .as_bool()
                || len == 0
            {
                return None;
            }
            let text = std::slice::from_raw_parts(ptr as *const u16, len as usize);
            let text = wide(text).trim().to_string();
            (!text.is_empty()).then_some(text)
        }
    }

    trait EncodeWideNull {
        fn encode_wide_null(&self) -> Vec<u16>;
    }

    impl EncodeWideNull for std::ffi::OsStr {
        fn encode_wide_null(&self) -> Vec<u16> {
            use std::os::windows::ffi::OsStrExt;
            self.encode_wide().chain(Some(0)).collect()
        }
    }

    /// Cada proceso con su ejecutable, su CPU y su memoria.
    fn procs(state: &mut State) -> Procs {
        let (mut idle, mut kernel, mut user) =
            (FILETIME::default(), FILETIME::default(), FILETIME::default());
        unsafe {
            let _ = GetSystemTimes(Some(&mut idle), Some(&mut kernel), Some(&mut user));
        }
        let mut rows = Vec::new();
        let mut ticks = HashMap::new();
        unsafe {
            if let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) {
                let mut entry = PROCESSENTRY32W {
                    dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
                    ..Default::default()
                };
                if Process32FirstW(snap, &mut entry).is_ok() {
                    loop {
                        let pid = entry.th32ProcessID;
                        if pid != 0 {
                            let exe = wide(&entry.szExeFile).to_lowercase();
                            let stem = exe.trim_end_matches(".exe").to_string();
                            if !hidden_process(&stem) {
                                let (ram, cpu_ticks, path) = process_stats(pid);
                                ticks.insert(pid, cpu_ticks);
                                rows.push((stem, pid, path, cpu_ticks, ram));
                            }
                        }
                        if Process32NextW(snap, &mut entry).is_err() {
                            break;
                        }
                    }
                }
                let _ = CloseHandle(snap);
            }
        }
        let (idle, total) = (filetime(idle), filetime(kernel) + filetime(user));
        let previous = state.cpu_sample.replace((idle, total, ticks));
        let (cpu, per_pid) = match previous {
            Some((prev_idle, prev_total, prev_ticks)) => {
                let total_d = total.saturating_sub(prev_total) as f64;
                let idle_d = idle.saturating_sub(prev_idle) as f64;
                let cpu = if total_d > 0.0 {
                    ((total_d - idle_d).max(0.0) / total_d * 100.0) as f32
                } else {
                    0.0
                };
                let per: HashMap<u32, f32> = rows
                    .iter()
                    .filter_map(|(_, pid, _, t, _)| {
                        let before = prev_ticks.get(pid)?;
                        (total_d > 0.0)
                            .then(|| (*pid, (t.saturating_sub(*before) as f64 / total_d * 100.0) as f32))
                    })
                    .collect();
                (cpu, per)
            }
            None => (0.0, HashMap::new()),
        };
        let rows = rows
            .into_iter()
            .map(|(stem, pid, path, _, ram)| {
                let cpu = per_pid.get(&pid).copied().unwrap_or(0.0);
                (stem, pid, path, cpu, ram)
            })
            .collect();
        let mut apps = group(rows, 8);
        for app in &mut apps {
            if let Some(path) = app.path.clone() {
                app.name = product_name(state, &path);
            }
        }
        let mut mem = MEMORYSTATUSEX {
            dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
            ..Default::default()
        };
        unsafe {
            let _ = GlobalMemoryStatusEx(&mut mem);
        }
        Procs {
            cpu: cpu.clamp(0.0, 100.0),
            ram_used: mem.ullTotalPhys.saturating_sub(mem.ullAvailPhys),
            ram_total: mem.ullTotalPhys,
            apps,
        }
    }

    fn process_stats(pid: u32) -> (u64, u64, Option<PathBuf>) {
        unsafe {
            let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
                return (0, 0, None);
            };
            let mut counters = PROCESS_MEMORY_COUNTERS {
                cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
                ..Default::default()
            };
            let ram = if GetProcessMemoryInfo(handle, &mut counters, counters.cb).is_ok() {
                counters.WorkingSetSize as u64
            } else {
                0
            };
            let (mut create, mut exit, mut kernel, mut user) = (
                FILETIME::default(),
                FILETIME::default(),
                FILETIME::default(),
                FILETIME::default(),
            );
            let ticks = if GetProcessTimes(handle, &mut create, &mut exit, &mut kernel, &mut user).is_ok() {
                filetime(kernel) + filetime(user)
            } else {
                0
            };
            let mut buffer = [0u16; 1024];
            let mut len = buffer.len() as u32;
            let path = QueryFullProcessImageNameW(
                handle,
                PROCESS_NAME_WIN32,
                PWSTR(buffer.as_mut_ptr()),
                &mut len,
            )
            .ok()
            .map(|_| PathBuf::from(String::from_utf16_lossy(&buffer[..len as usize])));
            let _ = CloseHandle(handle);
            (ram, ticks, path)
        }
    }

    fn pids_for(stem: &str) -> Vec<u32> {
        let mut out = Vec::new();
        unsafe {
            if let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) {
                let mut entry = PROCESSENTRY32W {
                    dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
                    ..Default::default()
                };
                if Process32FirstW(snap, &mut entry).is_ok() {
                    loop {
                        let exe = wide(&entry.szExeFile).to_lowercase();
                        if exe.trim_end_matches(".exe") == stem {
                            out.push(entry.th32ProcessID);
                        }
                        if Process32NextW(snap, &mut entry).is_err() {
                            break;
                        }
                    }
                }
                let _ = CloseHandle(snap);
            }
        }
        out
    }

    struct Closing {
        pids: Vec<u32>,
        closed: usize,
    }

    unsafe extern "system" fn close_each(hwnd: HWND, data: LPARAM) -> BOOL {
        let state = &mut *(data.0 as *mut Closing);
        if IsWindowVisible(hwnd).as_bool() {
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            if state.pids.contains(&pid)
                && PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0)).is_ok()
            {
                state.closed += 1;
            }
        }
        BOOL(1)
    }

    /// Como la × de cada ventana: la app decide (y pregunta si hay algo sin
    /// guardar).
    fn close(stem: &str) -> usize {
        if hidden_process(stem) {
            return 0;
        }
        let mut state = Closing {
            pids: pids_for(stem),
            closed: 0,
        };
        if state.pids.is_empty() {
            return 0;
        }
        unsafe {
            let _ = EnumWindows(Some(close_each), LPARAM(&mut state as *mut Closing as isize));
        }
        state.closed
    }

    fn force(stem: &str) -> Result<usize, String> {
        if hidden_process(stem) {
            return Err(crate::i18n::t("pill.system.cantForce").into());
        }
        let own = std::process::id();
        let mut killed = 0;
        for pid in pids_for(stem).into_iter().filter(|&pid| pid != own) {
            unsafe {
                if let Ok(handle) = OpenProcess(PROCESS_TERMINATE, false, pid) {
                    if TerminateProcess(handle, 1).is_ok() {
                        killed += 1;
                    }
                    let _ = CloseHandle(handle);
                }
            }
        }
        if killed == 0 {
            Err(crate::i18n::tf("pill.system.forceFailed", &[("name", &pretty(stem))]))
        } else {
            Ok(killed)
        }
    }
}

/// macOS: Core Audio para el volumen, las salidas y el micrófono;
/// `DisplayServices` (el del teclado y el Centro de control) para el brillo;
/// `caffeinate` para mantener despierto; `pmset` para la batería y `ps`
/// para los procesos, agrupados por app (`.app`).
#[cfg(target_os = "macos")]
mod imp {
    use super::*;
    use crate::platform::coreaudio as ca;
    use std::process::{Child, Command, Stdio};

    struct State {
        caffeinate: Option<Child>,
        /// El nombre que muestra el Finder de cada app, por su clave.
        names: HashMap<String, String>,
        /// El volumen por app (taps de Core Audio, macOS 14.2+).
        mixer: crate::platform::audio_tap::Mixer,
        /// Las muestras de Rendimiento (CPU, red) entre lecturas.
        vitals: crate::system::mac::Sampler,
    }

    /// El `CGDirectDisplayID` de una pantalla del panel.
    fn display_number(id: DisplayId) -> Option<u32> {
        match id {
            DisplayId::Internal => builtin_display(),
            DisplayId::Monitor(display) => Some(display as u32),
        }
    }

    pub fn run(rx: mpsc::Receiver<Cmd>, outcomes: std::sync::Arc<std::sync::Mutex<Vec<Outcome>>>) {
        let mut state = State { caffeinate: None, names: HashMap::new(), mixer: Default::default(), vitals: Default::default() };
        while let Ok(first) = rx.recv() {
            let mut cmds = vec![first];
            while let Ok(more) = rx.try_recv() {
                cmds.push(more);
            }
            let b = batch(&cmds.iter().collect::<Vec<_>>());
            if let Some(m) = b.mute {
                if let Some(device) = ca::default_device(false) {
                    ca::set::<u32>(device, ca::Address::new(ca::MUTE, ca::SCOPE_OUTPUT), m as u32);
                }
            }
            if let Some(level) = b.volume {
                set_volume(level);
            }
            if let Some(id) = b.output.as_ref() {
                if let Err(error) = set_default_output(id) {
                    eprintln!("sistema: salida de audio: {error}");
                }
            }
            if let Some(m) = b.mic_mute {
                if let Some(device) = ca::default_device(true) {
                    ca::set::<u32>(device, ca::Address::new(ca::MUTE, ca::SCOPE_INPUT), m as u32);
                }
            }
            for (pid, level) in &b.apps {
                if let Err(error) = state.mixer.set(*pid as i32, *level) {
                    eprintln!("sistema: volumen de app: {error:?}");
                }
            }
            for (id, level) in &b.brightness {
                let display = match id {
                    DisplayId::Internal => builtin_display(),
                    DisplayId::Monitor(display) => Some(*display as u32),
                };
                if !display.is_some_and(|d| display_services::set(d, level.clamp(0.0, 1.0))) {
                    eprintln!("sistema: brillo: la pantalla {id:?} no deja cambiarlo");
                }
            }
            if let Some(on) = b.awake {
                set_awake(&mut state, on);
            }
            for stem in &b.close {
                let name = state.names.get(stem).cloned().unwrap_or_else(|| pretty(stem));
                let outcome = if close(stem, false) > 0 {
                    Ok(crate::i18n::tf("pill.system.closeRequested", &[("name", &name)]))
                } else {
                    Err(crate::i18n::tf("pill.system.noWindows", &[("name", &name)]))
                };
                outcomes.lock().map(|mut o| o.push(outcome)).ok();
            }
            for stem in &b.force {
                let name = state.names.get(stem).cloned().unwrap_or_else(|| pretty(stem));
                let killed = close(stem, true);
                let outcome = if killed > 0 {
                    Ok(crate::i18n::tf("pill.system.closedProcs", &[("name", &name), ("n", &killed)]))
                } else {
                    Err(crate::i18n::tf("pill.system.forceFailed", &[("name", &name)]))
                };
                outcomes.lock().map(|mut o| o.push(outcome)).ok();
            }
            use crate::system::mac;
            for (id, mode) in &b.modes {
                if let Some(display) = display_number(*id) {
                    report(&outcomes, mac::set_mode(display, mode));
                }
            }
            if let Some(display) = b.primary.and_then(display_number) {
                report(&outcomes, mac::set_primary(display));
            }
            if let Some(on) = b.mirror {
                report(&outcomes, mac::set_mirror(on));
            }
            if let Some(dark) = b.dark {
                report(&outcomes, set_dark(dark));
            }
            if b.connect {
                report(&outcomes, mac::connect());
            }
            if let Some(want) = b.want {
                let snapshot = read(&mut state, want);
                for cmd in cmds {
                    if let Cmd::Read(_, reply) = cmd {
                        let _ = reply.send(snapshot.clone());
                    }
                }
            }
        }
        if let Some(mut child) = state.caffeinate.take() {
            let _ = child.kill();
        }
    }

    fn read(state: &mut State, want: Want) -> Snapshot {
        let output = ca::default_device(false);
        let volume = output.and_then(volume_of).unwrap_or(0.0);
        let muted = output
            .and_then(|d| ca::get::<u32>(d, ca::Address::new(ca::MUTE, ca::SCOPE_OUTPUT)))
            .is_some_and(|m| m != 0);
        let mic_muted = ca::default_device(true).map(|d| {
            ca::get::<u32>(d, ca::Address::new(ca::MUTE, ca::SCOPE_INPUT)).is_some_and(|m| m != 0)
        });
        let awake = state.caffeinate.as_mut().is_some_and(|child| matches!(child.try_wait(), Ok(None)));
        let vitals = want.vitals.then(|| {
            let mut vitals = crate::system::mac::vitals(&mut state.vitals);
            (vitals.ram_used, vitals.ram_total) = memory();
            vitals
        });
        Snapshot {
            volume,
            muted,
            battery: battery(),
            awake,
            displays: displays(),
            outputs: outputs(),
            mic_muted,
            // Con el permiso de audio del sistema, por taps de Core Audio; sin
            // él la pestaña no se ofrece.
            audio: if want.audio { app_audio(state) } else { Vec::new() },
            procs: want.procs.then(|| procs(state)),
            screens: crate::system::mac::screens(),
            mirrored: want.displays && crate::system::mac::mirrored(),
            dark: want.displays.then(|| !crate::theme::windows_light()),
            vitals,
        }
    }

    /// Las apps con sonido abierto (macOS 14.2+), una fila por proceso, con
    /// el volumen que tienen en el mezclador. Vacío sin el permiso de audio
    /// del sistema: sin él no se puede cambiar.
    fn app_audio(state: &mut State) -> Vec<AppAudio> {
        use crate::platform::permissions::{self, Permission, State as Granted};
        if permissions::state(Permission::SystemAudio) != Granted::Granted {
            return Vec::new();
        }
        let own = std::process::id() as i32;
        let processes: Vec<_> = ca::processes().into_iter().filter(|p| p.output && p.pid > 0 && p.pid != own).collect();
        let alive: Vec<i32> = processes.iter().map(|p| p.pid).collect();
        if let Err(error) = state.mixer.tick(&alive) {
            eprintln!("sistema: mezclador: {error:?}");
        }
        let mut out: Vec<AppAudio> = Vec::new();
        for process in processes {
            if out.iter().any(|a| a.pid == process.pid as u32) {
                continue;
            }
            let path = app_path(process.pid);
            let name = path
                .as_deref()
                .and_then(crate::app_icon::display_name)
                .unwrap_or_else(|| crate::platform::running_name(process.pid, process.bundle.as_deref()));
            out.push(AppAudio { pid: process.pid as u32, name, path, volume: state.mixer.gain(process.pid) });
        }
        out.sort_by_key(|a| a.name.to_lowercase());
        out
    }

    /// La `.app` de un proceso, para su nombre y su ícono.
    fn app_path(pid: i32) -> Option<PathBuf> {
        use objc2::rc::Retained;
        use objc2::runtime::AnyObject;
        use objc2::{class, msg_send};
        use objc2_foundation::{NSString, NSURL};
        objc2::rc::autoreleasepool(|_| unsafe {
            let app: Option<Retained<AnyObject>> =
                msg_send![class!(NSRunningApplication), runningApplicationWithProcessIdentifier: pid];
            let url: Option<Retained<NSURL>> = app.and_then(|app| msg_send![&app, bundleURL]);
            let path: Option<Retained<NSString>> = url.and_then(|u| u.path());
            path.map(|p| PathBuf::from(p.to_string()))
        })
    }

    /// El volumen «virtual» (el de la barra de menús) o, si el dispositivo no
    /// tiene, el del canal 1.
    fn volume_of(device: ca::ObjectId) -> Option<f32> {
        let main = ca::Address::new(ca::VIRTUAL_MAIN_VOLUME, ca::SCOPE_OUTPUT);
        if ca::has(device, main) {
            return ca::get::<f32>(device, main);
        }
        let channel = ca::Address { selector: ca::VOLUME_SCALAR, scope: ca::SCOPE_OUTPUT, element: 1 };
        ca::get::<f32>(device, channel)
    }

    fn set_volume(level: f32) {
        let Some(device) = ca::default_device(false) else {
            return;
        };
        let level = level.clamp(0.0, 1.0);
        let main = ca::Address::new(ca::VIRTUAL_MAIN_VOLUME, ca::SCOPE_OUTPUT);
        if ca::has(device, main) && ca::settable(device, main) {
            ca::set::<f32>(device, main, level);
        } else {
            for element in [0, 1, 2] {
                let channel = ca::Address { selector: ca::VOLUME_SCALAR, scope: ca::SCOPE_OUTPUT, element };
                if ca::has(device, channel) && ca::settable(device, channel) {
                    ca::set::<f32>(device, channel, level);
                }
            }
        }
        // Subir el volumen quita el silencio, como las teclas del Mac.
        if level > 0.0 {
            ca::set::<u32>(device, ca::Address::new(ca::MUTE, ca::SCOPE_OUTPUT), 0);
        }
    }

    fn outputs() -> Vec<Output> {
        let default = ca::default_device(false);
        ca::list(ca::SYSTEM, ca::Address::new(ca::DEVICES, ca::SCOPE_GLOBAL))
            .into_iter()
            .filter(|&device| ca::has_streams(device, ca::SCOPE_OUTPUT))
            // Los agregados privados de los taps no son salidas para elegir.
            .filter(|&device| {
                !ca::string(device, ca::Address::new(ca::UID, ca::SCOPE_GLOBAL))
                    .is_some_and(|uid| uid.starts_with(crate::platform::audio_tap::AGGREGATE_PREFIX))
            })
            .filter_map(|device| {
                let id = ca::string(device, ca::Address::new(ca::UID, ca::SCOPE_GLOBAL))?;
                let name = ca::string(device, ca::Address::new(ca::NAME, ca::SCOPE_GLOBAL)).unwrap_or_else(|| id.clone());
                Some(Output { id, name, default: Some(device) == default })
            })
            .collect()
    }

    fn set_default_output(uid: &str) -> Result<(), String> {
        let device = ca::list(ca::SYSTEM, ca::Address::new(ca::DEVICES, ca::SCOPE_GLOBAL))
            .into_iter()
            .find(|&d| ca::string(d, ca::Address::new(ca::UID, ca::SCOPE_GLOBAL)).as_deref() == Some(uid))
            .ok_or(crate::i18n::t("pill.system.gone"))?;
        let ok = ca::set::<ca::ObjectId>(ca::SYSTEM, ca::Address::new(ca::DEFAULT_OUTPUT, ca::SCOPE_GLOBAL), device);
        // Que los sonidos del sistema también vayan por ahí, como en Ajustes.
        ca::set::<ca::ObjectId>(ca::SYSTEM, ca::Address::new(u32::from_be_bytes(*b"sOut"), ca::SCOPE_GLOBAL), device);
        ok.then_some(()).ok_or_else(|| crate::i18n::t("pill.system.coreAudioRejected").into())
    }

    fn set_awake(state: &mut State, on: bool) {
        if let Some(mut child) = state.caffeinate.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        if on {
            // `-w`: se apaga solo si la pill termina.
            state.caffeinate = Command::new("caffeinate")
                .args(["-di", "-w", &std::process::id().to_string()])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|error| eprintln!("sistema: caffeinate: {error}"))
                .ok();
        }
    }

    /// `pmset -g batt`: «'AC Power'» o «'Battery Power'» y el porcentaje.
    fn battery() -> Option<(u8, bool)> {
        let out = Command::new("pmset").args(["-g", "batt"]).output().ok()?;
        parse_pmset(&String::from_utf8_lossy(&out.stdout))
    }

    pub(super) fn parse_pmset(text: &str) -> Option<(u8, bool)> {
        let line = text.lines().find(|l| l.contains("InternalBattery"))?;
        let percent = line.split('\t').nth(1).unwrap_or(line).split('%').next()?.trim();
        let percent = percent.rsplit(|c: char| !c.is_ascii_digit()).next()?.parse::<u8>().ok()?;
        let plugged = text.contains("'AC Power'");
        Some((percent.min(100), plugged))
    }

    fn displays() -> Vec<Display> {
        #[link(name = "CoreGraphics", kind = "framework")]
        extern "C" {
            fn CGGetActiveDisplayList(max: u32, displays: *mut u32, count: *mut u32) -> i32;
            fn CGMainDisplayID() -> u32;
            fn CGDisplayIsBuiltin(display: u32) -> u32;
        }
        let mut ids = [0u32; 16];
        let mut count = 0u32;
        // SAFETY: búfer de 16 ids; consultas puras.
        unsafe {
            if CGGetActiveDisplayList(ids.len() as u32, ids.as_mut_ptr(), &mut count) != 0 {
                return Vec::new();
            }
            let main = CGMainDisplayID();
            let names = screen_names();
            ids[..count as usize]
                .iter()
                .map(|&id| {
                    let builtin = CGDisplayIsBuiltin(id) != 0;
                    let name = names
                        .iter()
                        .find(|(screen, _)| *screen == id)
                        .map(|(_, name)| name.clone())
                        .unwrap_or_else(|| if builtin { crate::i18n::t("pill.system.builtinDisplay").into() } else { crate::i18n::tf("pill.system.displayId", &[("id", &id)]) });
                    let (mode, modes) = crate::system::mac::modes(id);
                    Display {
                        // La del Mac es «Internal»: el panel le pone el ícono del notebook.
                        id: if builtin { DisplayId::Internal } else { DisplayId::Monitor(id as isize) },
                        name,
                        primary: id == main,
                        brightness: display_services::get(id),
                        device: id.to_string(),
                        mode,
                        modes,
                    }
                })
                .collect()
        }
    }

    fn builtin_display() -> Option<u32> {
        #[link(name = "CoreGraphics", kind = "framework")]
        extern "C" {
            fn CGGetActiveDisplayList(max: u32, displays: *mut u32, count: *mut u32) -> i32;
            fn CGDisplayIsBuiltin(display: u32) -> u32;
        }
        let mut ids = [0u32; 16];
        let mut count = 0u32;
        // SAFETY: búfer de 16 ids; consultas puras.
        unsafe {
            if CGGetActiveDisplayList(ids.len() as u32, ids.as_mut_ptr(), &mut count) != 0 {
                return None;
            }
            ids[..count as usize].iter().copied().find(|&id| CGDisplayIsBuiltin(id) != 0)
        }
    }

    /// El nombre de cada pantalla (`NSScreen.localizedName`) por su id.
    fn screen_names() -> Vec<(u32, String)> {
        use objc2::rc::Retained;
        use objc2::runtime::AnyObject;
        use objc2::{class, msg_send};
        use objc2_foundation::{NSArray, NSNumber, NSString};
        objc2::rc::autoreleasepool(|_| unsafe {
            let screens: Retained<NSArray<AnyObject>> = msg_send![class!(NSScreen), screens];
            let key = NSString::from_str("NSScreenNumber");
            screens
                .iter()
                .filter_map(|screen| {
                    let info: Retained<AnyObject> = msg_send![&*screen, deviceDescription];
                    let number: Option<Retained<NSNumber>> = msg_send![&info, objectForKey: &*key];
                    let name: Option<Retained<NSString>> = msg_send![&*screen, localizedName];
                    Some((number?.as_u32(), name?.to_string()))
                })
                .collect()
        })
    }

    /// El brillo por `DisplayServices` (privado, se carga al usarlo): anda en
    /// la pantalla del Mac y en las de Apple; las demás devuelven error.
    mod display_services {
        use std::sync::OnceLock;

        type Get = unsafe extern "C" fn(u32, *mut f32) -> i32;
        type Set = unsafe extern "C" fn(u32, f32) -> i32;

        extern "C" {
            fn dlopen(path: *const std::ffi::c_char, mode: i32) -> *mut std::ffi::c_void;
            fn dlsym(handle: *mut std::ffi::c_void, name: *const std::ffi::c_char) -> *mut std::ffi::c_void;
        }

        fn functions() -> Option<(Get, Set)> {
            static FUNCTIONS: OnceLock<Option<(usize, usize)>> = OnceLock::new();
            let (get, set) = (*FUNCTIONS.get_or_init(|| unsafe {
                let handle = dlopen(
                    c"/System/Library/PrivateFrameworks/DisplayServices.framework/DisplayServices".as_ptr(),
                    1,
                );
                if handle.is_null() {
                    return None;
                }
                let get = dlsym(handle, c"DisplayServicesGetBrightness".as_ptr());
                let set = dlsym(handle, c"DisplayServicesSetBrightness".as_ptr());
                (!get.is_null() && !set.is_null()).then_some((get as usize, set as usize))
            }))?;
            // SAFETY: las firmas son las de `DisplayServices`.
            unsafe { Some((std::mem::transmute::<usize, Get>(get), std::mem::transmute::<usize, Set>(set))) }
        }

        pub fn get(display: u32) -> Option<f32> {
            let (get, _) = functions()?;
            let mut value = 0f32;
            // SAFETY: `value` vive durante la llamada.
            (unsafe { get(display, &mut value) } == 0).then_some(value.clamp(0.0, 1.0))
        }

        pub fn set(display: u32, level: f32) -> bool {
            functions().is_some_and(|(_, set)| unsafe { set(display, level) } == 0)
        }
    }

    /// La app (`.app`) de un ejecutable, si tiene: su carpeta y su clave.
    pub(super) fn bundle_of(exe: &str) -> Option<(String, String)> {
        let end = exe.find(".app/")? + 4;
        let bundle = &exe[..end];
        let name = bundle.rsplit('/').next()?.trim_end_matches(".app");
        Some((bundle.to_string(), name.to_lowercase()))
    }

    /// Lo que no se ofrece cerrar: lo de fuera de un `.app` (demonios y
    /// herramientas) y lo de sistema, salvo las apps de Apple.
    pub(super) fn system_bundle(bundle: &str) -> bool {
        (bundle.starts_with("/System/") && !bundle.starts_with("/System/Applications/") && !bundle.starts_with("/System/Volumes/Preboot/Cryptexes/App/System/Applications/"))
            || bundle.starts_with("/Library/Apple/")
            || bundle.starts_with("/usr/")
    }

    fn procs(state: &mut State) -> Procs {
        let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1) as f32;
        let out = Command::new("ps").args(["-axww", "-o", "pid=,pcpu=,rss=,comm="]).output();
        let text = out.map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
        let mut rows = Vec::new();
        let mut total_cpu = 0.0;
        for line in text.lines() {
            let mut parts = line.split_whitespace();
            let (Some(pid), Some(cpu), Some(rss)) = (parts.next(), parts.next(), parts.next()) else {
                continue;
            };
            let exe = parts.collect::<Vec<_>>().join(" ");
            let (Ok(pid), Ok(cpu), Ok(rss)) = (pid.parse::<u32>(), cpu.parse::<f32>(), rss.parse::<u64>()) else {
                continue;
            };
            total_cpu += cpu;
            let Some((bundle, stem)) = bundle_of(&exe) else {
                continue;
            };
            if system_bundle(&bundle) || pid == std::process::id() {
                continue;
            }
            // `ps` da el uso por núcleo; Windows (y el panel) por equipo.
            rows.push((stem, pid, Some(PathBuf::from(bundle)), cpu / cores, rss * 1024));
        }
        let mut apps = group(rows, 8);
        for app in &mut apps {
            let name = state.names.entry(app.stem.clone()).or_insert_with(|| {
                app.path.as_deref().and_then(crate::app_icon::display_name).unwrap_or_else(|| pretty(&app.stem))
            });
            app.name = name.clone();
        }
        let (ram_used, ram_total) = memory();
        Procs { cpu: (total_cpu / cores).clamp(0.0, 100.0), ram_used, ram_total, apps }
    }

    /// Memoria en uso como la cuenta el Monitor de Actividad: activa,
    /// residente y comprimida (`vm_stat`), y el total (`hw.memsize`).
    fn memory() -> (u64, u64) {
        let total = Command::new("sysctl")
            .args(["-n", "hw.memsize"])
            .output()
            .ok()
            .and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse::<u64>().ok())
            .unwrap_or(0);
        let text = Command::new("vm_stat").output().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
        (parse_vm_stat(&text), total)
    }

    pub(super) fn parse_vm_stat(text: &str) -> u64 {
        let page = text
            .lines()
            .next()
            .and_then(|l| l.split("page size of ").nth(1))
            .and_then(|r| r.split_whitespace().next())
            .and_then(|n| n.parse::<u64>().ok())
            .unwrap_or(16384);
        let pages = |key: &str| {
            text.lines()
                .find(|l| l.starts_with(key))
                .and_then(|l| l.rsplit(':').next())
                .and_then(|n| n.trim().trim_end_matches('.').parse::<u64>().ok())
                .unwrap_or(0)
        };
        let anonymous = pages("Anonymous pages").saturating_sub(pages("Pages purgeable"));
        let used = if anonymous > 0 { anonymous } else { pages("Pages active") };
        (used + pages("Pages wired down") + pages("Pages occupied by compressor")) * page
    }

    /// Cierra la app de `stem`: como Cmd+Q, o a la fuerza. Devuelve a
    /// cuántas apps (o procesos) llegó.
    fn close(stem: &str, force: bool) -> usize {
        use objc2::rc::Retained;
        use objc2::runtime::AnyObject;
        use objc2::{class, msg_send};
        use objc2_foundation::{NSArray, NSString, NSURL};
        let apps: Vec<i32> = objc2::rc::autoreleasepool(|_| unsafe {
            let workspace: Retained<AnyObject> = msg_send![class!(NSWorkspace), sharedWorkspace];
            let apps: Retained<NSArray<AnyObject>> = msg_send![&workspace, runningApplications];
            apps.iter()
                .filter(|app| {
                    let url: Option<Retained<NSURL>> = msg_send![&**app, bundleURL];
                    let path: Option<Retained<NSString>> = url.and_then(|u| u.path());
                    path.and_then(|p| bundle_of(&format!("{p}/"))).is_some_and(|(_, s)| s == stem)
                })
                .filter_map(|app| {
                    let ok: objc2::runtime::Bool = if force { msg_send![&*app, forceTerminate] } else { msg_send![&*app, terminate] };
                    let pid: i32 = msg_send![&*app, processIdentifier];
                    ok.as_bool().then_some(pid)
                })
                .collect()
        });
        apps.len()
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
mod imp {
    use super::*;
    pub fn run(rx: mpsc::Receiver<Cmd>, _: std::sync::Arc<std::sync::Mutex<Vec<Outcome>>>) {
        while let Ok(cmd) = rx.recv() {
            if let Cmd::Read(_, reply) = cmd {
                let _ = reply.send(Snapshot::default());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn de_una_tanda_queda_el_ultimo_valor() {
        let (tx, _rx) = futures::channel::oneshot::channel();
        let cmds = [
            Cmd::Volume(0.1),
            Cmd::AppVolume(7, 0.2),
            Cmd::Brightness(DisplayId::Internal, 0.3),
            Cmd::Volume(0.4),
            Cmd::AppVolume(7, 0.9),
            Cmd::Mute(true),
            Cmd::Volume(0.7),
            Cmd::Read(Want { audio: true, ..Want::default() }, tx),
        ];
        let b = batch(&cmds.iter().collect::<Vec<_>>());
        assert_eq!(b.volume, Some(0.7));
        assert_eq!(b.mute, Some(true));
        assert_eq!(b.apps.get(&7), Some(&0.9));
        assert_eq!(b.brightness.get(&DisplayId::Internal), Some(&0.3));
        assert_eq!(b.want, Some(Want { audio: true, ..Want::default() }));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn lecturas_de_macos() {
        let batt = "Now drawing from 'Battery Power'\n -InternalBattery-0 (id=123)\t87%; discharging; 4:10 remaining present: true\n";
        assert_eq!(imp::parse_pmset(batt), Some((87, false)));
        let ac = "Now drawing from 'AC Power'\n -InternalBattery-0 (id=1)\t100%; charged; 0:00 remaining\n";
        assert_eq!(imp::parse_pmset(ac), Some((100, true)));
        assert_eq!(imp::parse_pmset("Now drawing from 'AC Power'\n"), None);
        let vm = "Mach Virtual Memory Statistics: (page size of 16384 bytes)\nPages active: 10.\nPages wired down: 5.\nPages occupied by compressor: 2.\nAnonymous pages: 8.\nPages purgeable: 1.\n";
        assert_eq!(imp::parse_vm_stat(vm), (7 + 5 + 2) * 16384);
        assert_eq!(
            imp::bundle_of("/Applications/Google Chrome.app/Contents/Frameworks/Helper.app/Contents/MacOS/Helper"),
            Some(("/Applications/Google Chrome.app".into(), "google chrome".into()))
        );
        assert!(imp::system_bundle("/System/Library/CoreServices/Finder.app"));
        assert!(!imp::system_bundle("/System/Applications/Notes.app"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn rendimiento_de_macos() {
        use crate::system::mac;
        let sensors = [
            ("pACC MTR Temp Sensor2".to_string(), 54.0),
            ("eACC MTR Temp Sensor3".to_string(), 46.0),
            ("PMU tdie1".to_string(), 80.0),
            ("PMU tdev2".to_string(), -21.5),
            ("GPU MTR Temp Sensor1".to_string(), 30.0),
            ("GPU MTR Temp Sensor4".to_string(), 50.0),
        ];
        // En M1 mandan los núcleos (pACC/eACC); la GPU, el más caliente.
        assert_eq!(mac::summarize_temps(&sensors), (Some(50.0), Some(50.0)));
        let m2 = [("PMU tdie1".to_string(), 60.0), ("PMU tdie2".to_string(), 64.0)];
        assert_eq!(mac::summarize_temps(&m2), (Some(62.0), None));
        let battery = "  \"CycleCount\" = 12\n  \"DesignCapacity\" = 4000\n  \"BatteryData\" = {\"AppleRawMaxCapacity\"=3600}\n  \"Voltage\" = 12000\n  \"InstantAmperage\" = 18446744073709550616\n";
        let power = mac::parse_power(battery).expect("batería");
        assert_eq!(power.cycles, Some(12));
        assert_eq!(power.health, Some(0.9));
        // -1000 mA a 12 V: gastando 12 W.
        assert_eq!(power.watts, Some(-12.0));
        assert_eq!(mac::ioreg_number("\"Device Utilization %\"=39,", "Device Utilization %"), Some(39));
        assert_eq!(mac::parse_power("sin batería"), None);
    }

    #[test]
    fn la_red_parte_en_cero_y_no_da_picos_si_el_contador_vuelve_atras() {
        let mut prev = None;
        assert_eq!(net_rate(&mut prev, 1000, 500), (0.0, 0.0));
        if let Some((_, _, at)) = prev.as_mut() {
            *at -= std::time::Duration::from_secs(2);
        }
        let (down, up) = net_rate(&mut prev, 3000, 1500);
        assert!((down - 1000.0).abs() < 50.0 && (up - 500.0).abs() < 25.0, "{down} {up}");
        assert_eq!(net_rate(&mut prev, 10, 10), (0.0, 0.0));
    }

    #[test]
    fn procesos_agrupados_sin_los_del_sistema() {
        let rows = vec![
            ("chrome".into(), 1, None, 2.0, 100),
            ("chrome".into(), 2, None, 3.0, 300),
            ("svchost".into(), 3, None, 50.0, 900),
            ("code".into(), 4, None, 1.0, 2000),
        ];
        let apps = group(rows, 8);
        let names: Vec<_> = apps.iter().map(|a| (a.stem.as_str(), a.cpu, a.ram)).collect();
        assert_eq!(names, [("chrome", 5.0, 400), ("code", 1.0, 2000)]);
    }
}
