//! Los emuladores de la pestaña «Dispositivos»: qué hay instalado en la máquina, qué
//! falta y cómo conseguirlo, y encender o apagar un teléfono virtual.
//!
//! Android: el SDK donde lo deja Android Studio (o `ANDROID_HOME`), su emulador, los
//! teléfonos virtuales (AVD, `~/.android/avd`) y los que ya están encendidos (los
//! archivos de descubrimiento del emulador, también los que abrió Android Studio). Un
//! teléfono se enciende sin ventana (`-no-window`) con su API gRPC en un puerto libre:
//! su pantalla se ve en Atic Code (`emulator.rs`).
//!
//! Lo que falta se instala con un botón que corre el comando en la terminal integrada,
//! a la vista (licencias incluidas), y la pestaña se revisa sola hasta que queda listo
//! (como «Preparar Atic Code», `setup.rs`).
//!
//! iOS: el simulador viene con Xcode (solo en macOS); sin él se dice cómo conseguirlo.

use std::path::{Path, PathBuf};
use std::process::Command;

/// La imagen de sistema que se instala si no hay ninguna: Android 16 con Google APIs.
const SYSTEM_IMAGE_API: &str = "android-36";
/// El nombre del teléfono virtual que crea Atic.
pub const DEFAULT_AVD: &str = "atic-pixel";

/// Lo que hay de Android en la máquina.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Android {
    pub sdk: Option<PathBuf>,
    pub emulator: Option<PathBuf>,
    pub adb: Option<PathBuf>,
    /// `sdkmanager` (cmdline-tools): para bajar lo que falte.
    pub sdkmanager: Option<PathBuf>,
    /// Una imagen de sistema instalada (su carpeta relativa al SDK, `system-images/…/`).
    pub image: Option<String>,
    pub avds: Vec<Avd>,
    pub running: Vec<Running>,
    /// Android Studio está instalado (para abrirlo si hace falta).
    pub studio: Option<PathBuf>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Avd {
    pub name: String,
    /// La resolución de su pantalla (`hw.lcd.width`, `hw.lcd.height`).
    pub size: (u32, u32),
}

/// Un emulador encendido (por Atic o por otra herramienta) con su API gRPC.
#[derive(Clone, Debug, PartialEq)]
pub struct Running {
    pub avd: String,
    pub pid: u32,
    pub grpc: Option<u16>,
    /// El puerto de consola (`emulator-5554` para adb).
    pub console: Option<u16>,
}

/// Lo que falta para encender un teléfono virtual, en orden.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Missing {
    /// Ni SDK: se instala Android Studio (trae SDK, emulador y su asistente).
    Sdk,
    /// SDK sin emulador, sin imagen o sin teléfono: se bajan e instala uno.
    Phone,
}

impl Android {
    pub fn missing(&self) -> Option<Missing> {
        if self.sdk.is_none() {
            return Some(Missing::Sdk);
        }
        if self.emulator.is_none() || self.avds.is_empty() {
            return Some(Missing::Phone);
        }
        None
    }
}

/// Revisa todo (lee el disco: fuera del hilo de la interfaz).
pub fn check_android() -> Android {
    let sdk = sdk_root();
    let exe = |rel: &str| sdk.as_ref().map(|s| s.join(exe_name(rel))).filter(|p| p.is_file());
    let emulator = exe("emulator/emulator");
    let adb = exe("platform-tools/adb");
    let sdkmanager = sdk.as_ref().and_then(|s| find_sdkmanager(s));
    let image = sdk.as_ref().and_then(|s| system_image(s));
    Android {
        avds: list_avds(&avd_home()),
        running: list_running(),
        studio: studio_path(),
        sdk,
        emulator,
        adb,
        sdkmanager,
        image,
    }
}

fn exe_name(rel: &str) -> String {
    if cfg!(windows) {
        let bat = matches!(rel, "cmdline-tools/latest/bin/sdkmanager" | "cmdline-tools/latest/bin/avdmanager");
        format!("{rel}{}", if bat { ".bat" } else { ".exe" })
    } else {
        rel.to_string()
    }
}

/// Dónde está el SDK: `ANDROID_HOME`, `ANDROID_SDK_ROOT` o donde lo deja Android Studio.
pub fn sdk_root() -> Option<PathBuf> {
    let from_env = ["ANDROID_HOME", "ANDROID_SDK_ROOT"].iter().filter_map(|k| std::env::var_os(k)).map(PathBuf::from);
    let home = home();
    let defaults: Vec<PathBuf> = if cfg!(target_os = "macos") {
        home.iter().map(|h| h.join("Library/Android/sdk")).collect()
    } else if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(|l| PathBuf::from(l).join("Android").join("Sdk")).into_iter().collect()
    } else {
        home.iter().map(|h| h.join("Android/Sdk")).collect()
    };
    from_env.chain(defaults).find(|p| p.is_dir())
}

/// La carpeta de los teléfonos virtuales: `ANDROID_AVD_HOME` o `~/.android/avd`.
pub fn avd_home() -> PathBuf {
    std::env::var_os("ANDROID_AVD_HOME")
        .map(PathBuf::from)
        .or_else(|| home().map(|h| h.join(".android").join("avd")))
        .unwrap_or_default()
}

fn find_sdkmanager(sdk: &Path) -> Option<PathBuf> {
    let latest = sdk.join(exe_name("cmdline-tools/latest/bin/sdkmanager"));
    if latest.is_file() {
        return Some(latest);
    }
    // Otra versión de cmdline-tools (`cmdline-tools/17.0/bin`).
    let tools = std::fs::read_dir(sdk.join("cmdline-tools")).ok()?;
    let name = if cfg!(windows) { "sdkmanager.bat" } else { "sdkmanager" };
    tools.flatten().map(|e| e.path().join("bin").join(name)).find(|p| p.is_file())
}

/// La primera imagen de sistema instalada para esta arquitectura, más nueva primero.
fn system_image(sdk: &Path) -> Option<String> {
    let abi = host_abi();
    let mut found = Vec::new();
    for api in std::fs::read_dir(sdk.join("system-images")).ok()?.flatten() {
        for tag in std::fs::read_dir(api.path()).into_iter().flatten().flatten() {
            let dir = tag.path().join(abi);
            if dir.join("system.img").is_file() || dir.join("system.img.qcow2").is_file() || dir.is_dir() {
                found.push(format!("system-images/{}/{}/{abi}/", api.file_name().to_string_lossy(), tag.file_name().to_string_lossy()));
            }
        }
    }
    found.sort_by(|a, b| api_level(b).cmp(&api_level(a)));
    found.into_iter().next()
}

fn api_level(image: &str) -> u32 {
    image.split('/').nth(1).and_then(|api| api.trim_start_matches("android-").parse().ok()).unwrap_or(0)
}

pub fn host_abi() -> &'static str {
    if cfg!(target_arch = "aarch64") { "arm64-v8a" } else { "x86_64" }
}

/// Los teléfonos virtuales de `home` (`<nombre>.ini` + `<nombre>.avd/config.ini`).
pub fn list_avds(home: &Path) -> Vec<Avd> {
    let mut avds: Vec<Avd> = std::fs::read_dir(home)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("ini") {
                return None;
            }
            let name = path.file_stem()?.to_string_lossy().to_string();
            let ini = std::fs::read_to_string(&path).unwrap_or_default();
            let dir = ini_value(&ini, "path").map(PathBuf::from).unwrap_or_else(|| home.join(format!("{name}.avd")));
            let config = std::fs::read_to_string(dir.join("config.ini")).unwrap_or_default();
            let num = |key| ini_value(&config, key).and_then(|v| v.parse().ok());
            Some(Avd { name, size: (num("hw.lcd.width").unwrap_or(1080), num("hw.lcd.height").unwrap_or(2400)) })
        })
        .collect();
    avds.sort_by(|a, b| a.name.cmp(&b.name));
    avds
}

/// El `config.ini` del teléfono virtual `name`.
fn avd_config_path(name: &str) -> PathBuf {
    let home = avd_home();
    let ini = std::fs::read_to_string(home.join(format!("{name}.ini"))).unwrap_or_default();
    ini_value(&ini, "path").map(PathBuf::from).unwrap_or_else(|| home.join(format!("{name}.avd"))).join("config.ini")
}

/// El teléfono tiene teclado (`hw.keyboard=yes`): sin él, el emulador descarta las
/// teclas que le llegan por gRPC (Inicio, Atrás y lo escrito).
pub fn keyboard_enabled(name: &str) -> bool {
    let config = std::fs::read_to_string(avd_config_path(name)).unwrap_or_default();
    ini_value(&config, "hw.keyboard") == Some("yes")
}

/// Le activa el teclado antes de encenderlo (Android Studio lo trae así por defecto).
pub fn ensure_keyboard(name: &str) {
    let path = avd_config_path(name);
    let Ok(config) = std::fs::read_to_string(&path) else {
        return;
    };
    if ini_value(&config, "hw.keyboard") == Some("yes") {
        return;
    }
    let _ = std::fs::write(&path, with_keyboard(&config));
}

pub fn with_keyboard(config: &str) -> String {
    let mut out: Vec<String> = config.lines().filter(|l| l.split_once('=').map(|(k, _)| k.trim()) != Some("hw.keyboard")).map(str::to_string).collect();
    out.push("hw.keyboard=yes".into());
    out.join("\n") + "\n"
}

/// La tecla de Android (`KEYCODE_*`) de un nombre como los que manda la vista, para
/// `adb shell input keyevent` (un teléfono sin teclado).
pub fn android_keycode(key: &str) -> Option<u32> {
    Some(match key {
        "GoBack" => 4,
        "GoHome" => 3,
        "AppSwitch" => 187,
        "Enter" => 66,
        "Backspace" => 67,
        "Delete" => 112,
        "Tab" => 61,
        "ArrowLeft" => 21,
        "ArrowRight" => 22,
        "ArrowUp" => 19,
        "ArrowDown" => 20,
        "Home" => 122,
        "End" => 123,
        _ => return None,
    })
}

/// Una tecla o texto por `adb shell input` (para un teléfono sin teclado).
pub fn adb_input(adb: &Path, serial: &str, key: Option<u32>, text: Option<&str>) {
    let mut command = Command::new(adb);
    command.args(["-s", serial, "shell", "input"]);
    match (key, text) {
        (Some(code), _) => {
            command.args(["keyevent", &code.to_string()]);
        }
        (None, Some(text)) => {
            // `input text` no acepta espacios (van como `%s`); entre comillas para el shell del teléfono.
            let escaped = text.replace(' ', "%s").replace('\'', "'\\''");
            command.args(["text", &format!("'{escaped}'")]);
        }
        _ => return,
    }
    hide_console(&mut command);
    let _ = command.status();
}

/// `clave=valor` de un `.ini` del emulador.
pub fn ini_value<'a>(ini: &'a str, key: &str) -> Option<&'a str> {
    ini.lines().find_map(|line| line.split_once('=').filter(|(k, _)| k.trim() == key).map(|(_, v)| v.trim()))
}

/// Dónde deja el emulador sus archivos de descubrimiento (`pid_<n>.ini`).
fn discovery_dir() -> Option<PathBuf> {
    if cfg!(target_os = "macos") {
        home().map(|h| h.join("Library/Caches/TemporaryItems/avd/running"))
    } else if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(|l| PathBuf::from(l).join("Temp").join("avd").join("running"))
    } else {
        std::env::var_os("XDG_RUNTIME_DIR").map(|r| PathBuf::from(r).join("avd").join("running"))
    }
}

/// Los emuladores encendidos: sus archivos de descubrimiento con el proceso vivo.
pub fn list_running() -> Vec<Running> {
    let Some(dir) = discovery_dir() else {
        return Vec::new();
    };
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().to_string();
            let pid: u32 = name.strip_prefix("pid_")?.strip_suffix(".ini")?.parse().ok()?;
            let ini = std::fs::read_to_string(entry.path()).ok()?;
            parse_running(pid, &ini).filter(|_| alive(pid))
        })
        .collect()
}

pub fn parse_running(pid: u32, ini: &str) -> Option<Running> {
    Some(Running {
        avd: ini_value(ini, "avd.name").or_else(|| ini_value(ini, "avd.id"))?.to_string(),
        pid,
        grpc: ini_value(ini, "grpc.port").and_then(|p| p.parse().ok()),
        console: ini_value(ini, "port.serial").and_then(|p| p.parse().ok()),
    })
}

fn alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        // `kill -0` solo pregunta si el proceso existe.
        Command::new("kill").args(["-0", &pid.to_string()]).stderr(std::process::Stdio::null()).status().is_ok_and(|s| s.success())
    }
    #[cfg(not(unix))]
    {
        let mut command = Command::new("tasklist");
        command.args(["/FI", &format!("PID eq {pid}"), "/NH"]);
        hide_console(&mut command);
        command.output().is_ok_and(|o| String::from_utf8_lossy(&o.stdout).contains(&pid.to_string()))
    }
}

/// La carpeta del usuario.
fn home() -> Option<PathBuf> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from)
}

/// Sin la consola negra que abre Windows con cada proceso.
#[allow(unused_variables)]
fn hide_console(command: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
}

/// Un puerto TCP libre en la máquina, para la API gRPC del emulador.
pub fn free_port() -> Option<u16> {
    (8554..8654).find(|port| std::net::TcpListener::bind(("127.0.0.1", *port)).is_ok())
}

/// Enciende `avd` sin ventana, con su API gRPC en `grpc`. Sin audio ni animación de
/// arranque: menos CPU y arranca antes.
pub fn launch(emulator: &Path, avd: &str, grpc: u16) -> std::io::Result<std::process::Child> {
    let mut command = Command::new(emulator);
    command
        .args(["-avd", avd, "-no-window", "-no-audio", "-no-boot-anim", "-grpc", &grpc.to_string()])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    hide_console(&mut command);
    command.spawn()
}

/// Apaga el emulador guardando su estado (arranca rápido la próxima vez): `adb emu
/// kill`; sin adb, se termina el proceso.
pub fn stop(adb: Option<&Path>, running: &Running) {
    if let (Some(adb), Some(console)) = (adb, running.console) {
        let mut command = Command::new(adb);
        command.args(["-s", &format!("emulator-{console}"), "emu", "kill"]);
        hide_console(&mut command);
        if command.status().is_ok_and(|s| s.success()) {
            return;
        }
    }
    #[cfg(unix)]
    {
        let _ = Command::new("kill").arg(running.pid.to_string()).status();
    }
    #[cfg(windows)]
    {
        let mut command = Command::new("taskkill");
        command.args(["/PID", &running.pid.to_string(), "/T", "/F"]);
        hide_console(&mut command);
        let _ = command.status();
    }
}

/// Android Studio, si está instalado.
fn studio_path() -> Option<PathBuf> {
    let candidates: Vec<PathBuf> = if cfg!(target_os = "macos") {
        let mut list = vec![PathBuf::from("/Applications/Android Studio.app")];
        list.extend(home().map(|h| h.join("Applications/Android Studio.app")));
        list
    } else if cfg!(windows) {
        ["ProgramFiles", "LOCALAPPDATA"]
            .iter()
            .filter_map(|k| std::env::var_os(k))
            .map(|base| PathBuf::from(base).join("Android").join("Android Studio").join("bin").join("studio64.exe"))
            .collect()
    } else {
        vec![PathBuf::from("/opt/android-studio/bin/studio.sh")]
    };
    candidates.into_iter().find(|p| p.exists())
}

/// El Java que trae Android Studio, para `sdkmanager` (pide uno y la máquina puede no tenerlo).
fn studio_java(studio: &Path) -> Option<PathBuf> {
    let home = if cfg!(target_os = "macos") { studio.join("Contents/jbr/Contents/Home") } else { studio.parent()?.parent()?.join("jbr") };
    home.is_dir().then_some(home)
}

pub fn has_command(name: &str) -> bool {
    let mut command = Command::new(if cfg!(windows) { "where" } else { "which" });
    command.arg(name).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    hide_console(&mut command);
    command.status().is_ok_and(|s| s.success())
}

/// El comando que instala lo que falta, para correrlo en la terminal integrada (a la
/// vista: el usuario ve el avance y acepta las licencias). `None`: no hay cómo
/// hacerlo desde aquí (se abre la página de descarga).
pub fn install_command(android: &Android, missing: Missing) -> Option<String> {
    match missing {
        Missing::Sdk => {
            if cfg!(target_os = "macos") && has_command("brew") {
                Some("brew install --cask android-studio && open -a 'Android Studio'".into())
            } else if cfg!(windows) && has_command("winget") {
                Some("winget install --id Google.AndroidStudio -e".into())
            } else {
                None
            }
        }
        Missing::Phone => {
            let sdk = android.sdk.as_ref()?;
            let sdkmanager = android.sdkmanager.as_ref()?;
            let quote = |p: &Path| shell_quote(&p.display().to_string());
            let image = format!("system-images;{SYSTEM_IMAGE_API};google_apis;{}", host_abi());
            let java = android.studio.as_deref().and_then(studio_java);
            let set_java = match (java, cfg!(windows)) {
                (Some(java), false) => format!("export JAVA_HOME={}; ", quote(&java)),
                (Some(java), true) => format!("$env:JAVA_HOME={}; ", quote(&java)),
                (None, _) => String::new(),
            };
            let manager = quote(sdkmanager);
            let root = quote(sdk);
            let run = if cfg!(windows) { "& " } else { "" };
            Some(format!(
                "{set_java}{run}{manager} --sdk_root={root} --licenses; {run}{manager} --sdk_root={root} \"emulator\" \"platform-tools\" \"{image}\""
            ))
        }
    }
}

/// Entre comillas simples para zsh, bash y PowerShell.
fn shell_quote(text: &str) -> String {
    if cfg!(windows) { format!("'{}'", text.replace('\'', "''")) } else { format!("'{}'", text.replace('\'', "'\\''")) }
}

/// La página para bajar Android Studio a mano.
pub const STUDIO_URL: &str = "https://developer.android.com/studio";

/// Crea un teléfono virtual con la imagen instalada, escribiendo sus archivos (lo que
/// hace `avdmanager`, sin pedir Java): un Pixel de 1080×2400.
pub fn create_avd(home: &Path, sdk: &Path, image: &str, name: &str) -> std::io::Result<()> {
    let dir = home.join(format!("{name}.avd"));
    std::fs::create_dir_all(&dir)?;
    let tag = image.split('/').nth(2).unwrap_or("google_apis");
    let playstore = tag.contains("playstore");
    std::fs::write(home.join(format!("{name}.ini")), avd_ini(&dir, sdk, name))?;
    std::fs::write(dir.join("config.ini"), avd_config(image, name, tag, playstore))?;
    Ok(())
}

fn avd_ini(dir: &Path, sdk: &Path, name: &str) -> String {
    let _ = sdk;
    format!("avd.ini.encoding=UTF-8\npath={}\npath.rel=avd/{name}.avd\ntarget=android-36\n", dir.display())
}

pub fn avd_config(image: &str, name: &str, tag: &str, playstore: bool) -> String {
    let abi = image.trim_end_matches('/').rsplit('/').next().unwrap_or("arm64-v8a");
    format!(
        "AvdId={name}\navd.ini.displayname={name}\nabi.type={abi}\nhw.cpu.arch={arch}\nhw.cpu.ncore=4\nhw.ramSize=2048\n\
         hw.lcd.width=1080\nhw.lcd.height=2400\nhw.lcd.density=420\nhw.keyboard=yes\nhw.gpu.enabled=yes\nhw.gpu.mode=auto\n\
         image.sysdir.1={image}\ntag.id={tag}\ntag.ids={tag}\nPlayStore.enabled={ps}\nfastboot.forceColdBoot=no\nfastboot.forceFastBoot=yes\n\
         disk.dataPartition.size=6G\nhw.initialOrientation=portrait\nshowDeviceFrame=no\n",
        arch = if abi.starts_with("arm64") { "arm64" } else { "x86_64" },
        ps = if playstore { "yes" } else { "no" },
    )
}

// --- iOS ---------------------------------------------------------------------------

/// Lo que hay del simulador de iOS (solo macOS).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Ios {
    /// Xcode completo (no solo las Command Line Tools): trae el simulador.
    pub xcode: bool,
    pub simulators: Vec<Simulator>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Simulator {
    pub name: String,
    pub udid: String,
    pub booted: bool,
}

pub fn check_ios() -> Ios {
    if !cfg!(target_os = "macos") {
        return Ios::default();
    }
    let xcode = Command::new("xcode-select")
        .arg("-p")
        .output()
        .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).contains(".app/Contents/Developer"));
    if !xcode {
        return Ios::default();
    }
    let simulators = Command::new("xcrun")
        .args(["simctl", "list", "devices", "available", "--json"])
        .output()
        .ok()
        .map(|o| parse_simulators(&String::from_utf8_lossy(&o.stdout)))
        .unwrap_or_default();
    Ios { xcode, simulators }
}

pub fn parse_simulators(json: &str) -> Vec<Simulator> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(json) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (runtime, list) in value.get("devices").and_then(|d| d.as_object()).into_iter().flatten() {
        if !runtime.contains("iOS") {
            continue;
        }
        for device in list.as_array().into_iter().flatten() {
            let text = |k| device.get(k).and_then(serde_json::Value::as_str).unwrap_or_default().to_string();
            if text("name").starts_with("iPhone") {
                out.push(Simulator { name: text("name"), udid: text("udid"), booted: text("state") == "Booted" });
            }
        }
    }
    out
}

/// Enciende el simulador y abre Simulator.app con él.
pub fn boot_simulator(udid: &str) {
    let _ = Command::new("xcrun").args(["simctl", "boot", udid]).status();
    let _ = Command::new("open").args(["-a", "Simulator", "--args", "-CurrentDeviceUDID", udid]).status();
}

/// Xcode en la App Store.
pub const XCODE_URL: &str = "macappstore://apps.apple.com/app/id497799835";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lee_un_emulador_encendido_de_su_archivo() {
        let ini = "avd.name=atic-api36\nport.serial=5554\ngrpc.port=8554\ngrpc.allowlist=/x.json\n";
        assert_eq!(parse_running(42, ini), Some(Running { avd: "atic-api36".into(), pid: 42, grpc: Some(8554), console: Some(5554) }));
        // Sin gRPC (lo abrió otra herramienta sin `-grpc`): se ve, pero no se puede mostrar.
        assert_eq!(parse_running(7, "avd.id=pixel\n").map(|r| r.grpc), Some(None));
    }

    #[test]
    fn lista_los_telefonos_virtuales_con_su_pantalla() {
        let home = std::env::temp_dir().join(format!("atic-avd-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        create_avd(&home, Path::new("/sdk"), "system-images/android-36/google_apis/arm64-v8a/", DEFAULT_AVD).unwrap();
        let avds = list_avds(&home);
        assert_eq!(avds, vec![Avd { name: DEFAULT_AVD.into(), size: (1080, 2400) }]);
        let config = std::fs::read_to_string(home.join(format!("{DEFAULT_AVD}.avd/config.ini"))).unwrap();
        assert_eq!(ini_value(&config, "image.sysdir.1"), Some("system-images/android-36/google_apis/arm64-v8a/"));
        assert_eq!(ini_value(&config, "PlayStore.enabled"), Some("no"));
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn el_teclado_se_activa_sin_tocar_lo_demas() {
        let config = "hw.lcd.width=1080\nhw.keyboard=no\nhw.keyboard.charmap=qwerty2\n";
        let fixed = with_keyboard(config);
        assert_eq!(ini_value(&fixed, "hw.keyboard"), Some("yes"));
        assert_eq!(ini_value(&fixed, "hw.keyboard.charmap"), Some("qwerty2"));
        assert_eq!(ini_value(&fixed, "hw.lcd.width"), Some("1080"));
        assert_eq!(android_keycode("GoHome"), Some(3));
    }

    #[test]
    fn dice_que_falta_en_orden() {
        let mut android = Android::default();
        assert_eq!(android.missing(), Some(Missing::Sdk));
        android.sdk = Some("/sdk".into());
        assert_eq!(android.missing(), Some(Missing::Phone));
        android.emulator = Some("/sdk/emulator/emulator".into());
        android.avds = vec![Avd { name: "a".into(), size: (1080, 2400) }];
        assert_eq!(android.missing(), None);
    }

    #[test]
    fn lee_los_iphone_de_simctl() {
        let json = r#"{"devices":{"com.apple.CoreSimulator.SimRuntime.iOS-18-0":[{"name":"iPhone 16","udid":"A","state":"Booted"},{"name":"iPad Air","udid":"B","state":"Shutdown"}],"com.apple.CoreSimulator.SimRuntime.watchOS-11":[{"name":"Apple Watch","udid":"C","state":"Shutdown"}]}}"#;
        assert_eq!(parse_simulators(json), vec![Simulator { name: "iPhone 16".into(), udid: "A".into(), booted: true }]);
    }
}
