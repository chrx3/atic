//! Congelamientos: si el hilo principal deja de responder, se guarda qué
//! estaba haciendo y, si no vuelve, Atic se reinicia sola.
//!
//! Un hilo aparte le pregunta al principal si está vivo:
//! - macOS: le encarga una tarea mínima en la cola principal de GCD. La cola
//!   se atiende también con un menú abierto o arrastrando una ventana (los
//!   bucles de eventos anidados), así que eso no cuenta como congelado; antes
//!   el pulso salía del tick de la pill y un menú abierto lo dejaba quieto
//!   (dos de los cuatro `congelado-*.txt` eran eso).
//! - Windows: `IsHungAppWindow`, la misma medida del sistema que pone «No
//!   responde» en el título.
//!
//! Con 5 s sin respuesta se guarda dónde quedó (`/usr/bin/sample`, en macOS)
//! en `congelado-<fecha>.txt`, junto al log. Con 15 s, Atic se reinicia: una
//! app trabada con la mira de captura abierta tapa toda la pantalla y no deja
//! hacer clic en nada, y desde adentro no hay otra salida (el menú del ícono
//! y los atajos dependen del mismo hilo). Al volver, un aviso lo cuenta.
//!
//! No se reinicia dos veces en 10 minutos (si se traba al arrancar, no queda
//! en un ciclo), ni en la pill de desarrollo, salvo con
//! `PILL_FREEZE_RESTART=1`. `PILL_FREEZE_TEST=<s>` traba el hilo principal
//! esos segundos, 10 s después de arrancar, para probarlo.
//!
//! El reloj monótono de macOS no avanza con el equipo dormido, así que dormir
//! no cuenta como congelamiento.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

/// Sin respuesta por esto, se guarda dónde quedó.
const STUCK: Duration = Duration::from_secs(5);
/// Sin respuesta por esto, se reinicia.
const RESTART: Duration = Duration::from_secs(15);
/// No se reinicia de nuevo antes de esto.
const RESTART_AGAIN: Duration = Duration::from_secs(10 * 60);
const CHECK: Duration = Duration::from_secs(1);

static START: OnceLock<Instant> = OnceLock::new();
/// Milisegundos desde `START` de la última respuesta del hilo principal.
static ANSWERED: AtomicU64 = AtomicU64::new(0);

fn now_ms() -> u64 {
    START.get_or_init(Instant::now).elapsed().as_millis() as u64
}

/// El hilo principal sigue vivo (lo llama el tick de la pill; también cuenta
/// como respuesta).
pub fn beat() {
    ANSWERED.store(now_ms(), Ordering::Relaxed);
}

/// La marca de un reinicio por congelamiento: `pendiente` hasta que la
/// instancia nueva avisa; su fecha frena un segundo reinicio seguido.
fn marker() -> Option<PathBuf> {
    crate::paths::file(".reinicio-por-congelamiento")
}

/// Lo que hace falta para volver a abrir Atic, leído al arrancar (en macOS,
/// la `.app`; preguntarlo con el hilo principal trabado no es seguro).
static RELAUNCH: OnceLock<Option<PathBuf>> = OnceLock::new();

pub fn start() {
    beat();
    #[cfg(target_os = "macos")]
    let target = crate::platform::bundle_path();
    #[cfg(not(target_os = "macos"))]
    let target = std::env::current_exe().ok();
    let _ = RELAUNCH.set(target);
    announce_restart();
    let spawned = std::thread::Builder::new().name("vigia-congelado".into()).spawn(watch);
    if let Err(error) = spawned {
        tracing::warn!(%error, "no arrancó el vigía de congelamientos");
    }
}

fn watch() {
    // Uno por congelamiento: hasta que vuelva a responder no se repite.
    let mut reported = false;
    loop {
        ask();
        std::thread::sleep(CHECK);
        let quiet = quiet_ms();
        if quiet < STUCK.as_millis() as u64 {
            reported = false;
            continue;
        }
        if !reported {
            reported = true;
            save_sample(quiet);
        }
        if quiet >= RESTART.as_millis() as u64 {
            restart(quiet);
        }
    }
}

/// Le pregunta al hilo principal si está vivo; contesta en `ANSWERED`.
#[cfg(target_os = "macos")]
fn ask() {
    use std::ffi::c_void;
    extern "C" {
        static _dispatch_main_q: c_void;
        fn dispatch_async_f(queue: *const c_void, context: *mut c_void, work: extern "C" fn(*mut c_void));
    }
    extern "C" fn answer(_: *mut c_void) {
        beat();
    }
    // SAFETY: la cola principal vive todo el proceso; `answer` no usa el contexto.
    unsafe { dispatch_async_f(&_dispatch_main_q, std::ptr::null_mut(), answer) };
}

/// En Windows la respuesta es del sistema: mientras ninguna ventana de Atic
/// esté «colgada», el hilo atiende sus mensajes.
#[cfg(windows)]
fn ask() {
    use windows::core::BOOL;
    use windows::Win32::Foundation::{HWND, LPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, GetWindowThreadProcessId, IsHungAppWindow, IsWindowVisible};
    unsafe extern "system" fn each(hwnd: HWND, data: LPARAM) -> BOOL {
        let hung = &mut *(data.0 as *mut bool);
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == std::process::id() && IsWindowVisible(hwnd).as_bool() && IsHungAppWindow(hwnd).as_bool() {
            *hung = true;
            return BOOL(0);
        }
        BOOL(1)
    }
    let mut hung = false;
    // SAFETY: `hung` vive durante toda la enumeración.
    unsafe {
        let _ = EnumWindows(Some(each), LPARAM(&mut hung as *mut bool as isize));
    }
    if !hung {
        beat();
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
fn ask() {}

fn quiet_ms() -> u64 {
    now_ms().saturating_sub(ANSWERED.load(Ordering::Relaxed))
}

/// Dónde quedó trabado, junto al log (macOS: `sample` de 3 s).
fn save_sample(quiet: u64) {
    let Some(dir) = crate::paths::logs_dir() else {
        return;
    };
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default();
    let file = dir.join(format!("congelado-{stamp}.txt"));
    tracing::error!(ms = quiet, archivo = %file.display(), "la pill no responde: guardando dónde quedó");
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("/usr/bin/sample")
        .arg(std::process::id().to_string())
        .arg("3")
        .arg("-file")
        .arg(&file)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

fn restart_allowed() -> bool {
    !cfg!(debug_assertions) || std::env::var("PILL_FREEZE_RESTART").is_ok_and(|v| !v.is_empty() && v != "0")
}

/// Sin reiniciar hace poco: la fecha de la marca.
fn restarted_recently() -> bool {
    marker()
        .and_then(|path| std::fs::metadata(path).ok())
        .and_then(|meta| meta.modified().ok())
        .and_then(|at| at.elapsed().ok())
        .is_some_and(|ago| ago < RESTART_AGAIN)
}

/// Cierra este proceso de golpe y deja lista la vuelta de Atic. No vuelve si
/// no se puede (desarrollo, otro reinicio reciente o sin saber qué abrir).
fn restart(quiet: u64) {
    if !restart_allowed() {
        return;
    }
    if restarted_recently() {
        tracing::error!(ms = quiet, "la pill no responde, pero ya se reinició hace poco: no se reinicia de nuevo");
        // Una vez basta: hasta que responda no se vuelve a intentar.
        std::thread::sleep(RESTART_AGAIN);
        return;
    }
    let Some(target) = RELAUNCH.get().cloned().flatten() else {
        return;
    };
    tracing::error!(ms = quiet, "la pill no responde: se reinicia");
    if let Some(path) = marker() {
        let _ = std::fs::write(path, "pendiente");
    }
    #[cfg(target_os = "macos")]
    {
        if crate::platform::reopen_after_exit(&target).is_err() {
            return;
        }
        // Sin pasar por el cierre normal: espera al hilo trabado.
        // SAFETY: termina el proceso; no vuelve.
        unsafe { libc::_exit(3) };
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let script = format!(
            "Wait-Process -Id {} -ErrorAction SilentlyContinue; Start-Process -FilePath '{}'",
            std::process::id(),
            target.display().to_string().replace('\'', "''")
        );
        let mut command = std::process::Command::new("powershell");
        // La Atic que vuelve no hereda los `PILL_*` de pruebas.
        for (key, _) in std::env::vars_os() {
            if key.to_string_lossy().starts_with("PILL_") {
                command.env_remove(key);
            }
        }
        let spawned = command
            .args(["-NoProfile", "-WindowStyle", "Hidden", "-Command", &script])
            .creation_flags(CREATE_NO_WINDOW)
            .spawn();
        if spawned.is_err() {
            return;
        }
        // SAFETY: termina este proceso; no vuelve.
        unsafe {
            use windows::Win32::System::Threading::{GetCurrentProcess, TerminateProcess};
            let _ = TerminateProcess(GetCurrentProcess(), 3);
        }
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    let _ = target;
}

/// Si la vez anterior se reinició por un congelamiento, se cuenta (una vez).
fn announce_restart() {
    let Some(path) = marker() else {
        return;
    };
    if std::fs::read_to_string(&path).ok().as_deref() != Some("pendiente") {
        return;
    }
    let _ = std::fs::write(&path, "avisado");
    tracing::warn!("se reinició por un congelamiento");
    // Un momento: el ícono de la barra (y sus avisos) recién está partiendo.
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(3));
        crate::platform::tray_icon::notify(
            crate::i18n::t("pill.freeze.restartedTitle"),
            crate::i18n::t("pill.freeze.restartedBody"),
            false,
            None,
        );
    });
}

