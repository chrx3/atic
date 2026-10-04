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
}

/// Qué leer además de lo básico: enumerar procesos cuesta, y solo hace falta
/// con su pestaña abierta.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Want {
    pub audio: bool,
    pub procs: bool,
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
            Cmd::Read(want, _) => {
                let prev = out.want.unwrap_or_default();
                out.want = Some(Want {
                    audio: prev.audio || want.audio,
                    procs: prev.procs || want.procs,
                });
            }
        }
    }
    out
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
                    Ok(format!("Se pidió cerrar {}", pretty(stem)))
                } else {
                    Err(format!("{} no tiene ventanas: usa Forzar", pretty(stem)))
                };
                outcomes.lock().map(|mut o| o.push(outcome)).ok();
            }
            for stem in &b.force {
                let outcome = force(stem)
                    .map(|n| format!("{} cerrado ({n} procesos)", pretty(stem)));
                outcomes.lock().map(|mut o| o.push(outcome)).ok();
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
                        .unwrap_or_else(|| "Salida de audio".into());
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
                .unwrap_or_else(|| format!("App {pid}"));
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

    struct Monitors(Vec<(HMONITOR, String, bool)>);

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
            list.0.push((monitor, name, info.monitorInfo.dwFlags & 1 != 0));
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
        for (index, (monitor, name, primary)) in monitors.0.into_iter().enumerate() {
            let ddc = ddc_brightness(monitor);
            let (id, brightness, label) = if let Some(level) = ddc {
                (DisplayId::Monitor(monitor.0 as isize), Some(level), monitor_name(&name, index))
            } else if internal.is_some() && !internal_used {
                internal_used = true;
                (DisplayId::Internal, internal, "Pantalla del equipo".to_string())
            } else {
                (DisplayId::Monitor(monitor.0 as isize), None, monitor_name(&name, index))
            };
            out.push(Display {
                id,
                name: label,
                primary,
                brightness,
            });
        }
        // La que tiene control primero: es la del slider grande.
        out.sort_by_key(|d| (d.brightness.is_none(), !d.primary));
        out
    }

    fn monitor_name(name: &str, index: usize) -> String {
        if name.is_empty() || name.to_lowercase().contains("generic") || name.contains("PnP") {
            format!("Monitor {}", index + 1)
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
            return Err("esa app no se puede forzar".into());
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
            Err(format!("No se pudo forzar {}", pretty(stem)))
        } else {
            Ok(killed)
        }
    }
}

#[cfg(not(windows))]
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
            Cmd::Read(Want { audio: true, procs: false }, tx),
        ];
        let b = batch(&cmds.iter().collect::<Vec<_>>());
        assert_eq!(b.volume, Some(0.7));
        assert_eq!(b.mute, Some(true));
        assert_eq!(b.apps.get(&7), Some(&0.9));
        assert_eq!(b.brightness.get(&DisplayId::Internal), Some(&0.3));
        assert_eq!(b.want, Some(Want { audio: true, procs: false }));
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
