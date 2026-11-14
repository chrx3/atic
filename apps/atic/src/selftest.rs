//! `PILL_SELFTEST=arranque`: la autoprueba de arranque. El release la corre
//! con el ejecutable ya empaquetado (`.app` o instalado) antes de publicar.
//!
//! Inicializa lo esencial y lo comprueba: la configuración, los textos, las
//! Novedades de esta versión, las APIs nativas (`mac_smoke.rs` en macOS y su
//! par de Windows aquí), el agente de Atic Code y `atic-mcp` si vienen en el
//! paquete, y una ventana con el overlay si hay pantalla. Escribe un JSON con
//! el resultado y termina: 0 si todo salió bien, 1 si algo falló y 2 si se
//! pasó del plazo.
//!
//! Corre al lado de la pill abierta sin tocarla: no toma el candado de
//! instancia única, no cierra la instalada, no escribe en `config.json` ni en
//! el log de la pill, no registra atajos, no pide permisos (solo consulta su
//! estado) y el agente y `atic-mcp` que levanta son propios, por stdio.
//!
//! Variables:
//! - `PILL_SELFTEST_SALIDA`: dónde escribir el JSON (por omisión,
//!   `selftest.json` en la carpeta de logs de Atic). También sale por stdout.
//! - `PILL_SELFTEST_PLAZO`: segundos antes de rendirse (120 por omisión).
//! - `PILL_SELFTEST_EXIGE`: comprobaciones opcionales que deben correr, por
//!   ejemplo `sidecar,mcp,ventana`: si falta lo que necesitan, fallan en vez
//!   de quedar omitidas.
//! - `PILL_SELFTEST_OMITE`: comprobaciones que no se hacen (`ventana`…).
//!   En un Mac sin Metal (algún runner Intel), gpui termina el proceso al
//!   abrir la ventana: `PILL_SELFTEST_OMITE=ventana` comprueba el resto.
//!
//! El JSON se escribe también antes de cada comprobación, con `en_curso`: si
//! el proceso muere sin pasar por `finish` (gpui sale con `exit` sin Metal,
//! por ejemplo), queda el informe hasta ahí y qué estaba corriendo.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{mpsc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use gpui::{
    div, point, prelude::*, px, size, App, Application, AssetSource, Bounds, Context, SharedString, Window,
    WindowBackgroundAppearance, WindowBounds, WindowKind, WindowOptions,
};
use serde_json::{json, Value};

/// Lo que se va anotando; si se vence el plazo, se escribe lo que haya.
static RESULTS: Mutex<Vec<Value>> = Mutex::new(Vec::new());
static STARTED: OnceLock<Instant> = OnceLock::new();
/// Cuadros que alcanzó a dibujar la ventana de prueba.
static FRAMES: AtomicUsize = AtomicUsize::new(0);
/// Una sola escritura del JSON a la vez; tras el informe final, ninguna más
/// (el hilo del plazo y el principal pueden llegar juntos).
static WRITING: Mutex<()> = Mutex::new(());
static FINISHED: AtomicBool = AtomicBool::new(false);

/// El modo pedido en `PILL_SELFTEST`, si hay uno.
pub fn requested() -> Option<String> {
    std::env::var("PILL_SELFTEST").ok().filter(|mode| !mode.is_empty())
}

/// Cómo terminó una comprobación que pudo correr o no.
enum Step {
    Done(String),
    /// Algo no anda, pero no frena el release (p. ej. el OCR sin el idioma).
    #[cfg_attr(not(any(windows, target_os = "macos")), allow(dead_code))]
    Warn(String),
    Skipped(String),
}

fn listed(var: &str, name: &str) -> bool {
    std::env::var(var).is_ok_and(|list| list.split(',').any(|item| item.trim() == name))
}

fn elapsed_ms() -> u128 {
    STARTED.get().map_or(0, |started| started.elapsed().as_millis())
}

fn record(name: &str, status: &str, ms: u128, detail: &str) {
    if let Ok(mut results) = RESULTS.lock() {
        results.push(json!({ "nombre": name, "estado": status, "ms": ms, "detalle": detail }));
    }
}

fn panic_text(payload: Box<dyn std::any::Any + Send>) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|text| text.to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "pánico".into())
}

/// Corre una comprobación anotando cuánto tardó; un pánico cuenta como falla.
fn check(name: &str, run: impl FnOnce() -> Result<Step, String>) {
    if listed("PILL_SELFTEST_OMITE", name) {
        record(name, "omitido", 0, "PILL_SELFTEST_OMITE");
        return;
    }
    write_report(false, Some(name));
    let started = Instant::now();
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(run)).unwrap_or_else(|payload| Err(panic_text(payload)));
    let ms = started.elapsed().as_millis();
    match outcome {
        Ok(Step::Done(detail)) => record(name, "ok", ms, &detail),
        Ok(Step::Warn(detail)) => record(name, "aviso", ms, &detail),
        Ok(Step::Skipped(why)) if listed("PILL_SELFTEST_EXIGE", name) => record(name, "falla", ms, &format!("exigida, pero {why}")),
        Ok(Step::Skipped(why)) => record(name, "omitido", ms, &why),
        Err(why) => record(name, "falla", ms, &why),
    }
}

fn output_path() -> PathBuf {
    std::env::var_os("PILL_SELFTEST_SALIDA")
        .map(PathBuf::from)
        .or_else(|| crate::paths::logs_dir().map(|dir| dir.join("selftest.json")))
        .unwrap_or_else(|| std::env::temp_dir().join("atic-selftest.json"))
}

/// Escribe el JSON con lo que haya. Con `running`, es un informe parcial:
/// `ok` en falso, sin `codigo` y con `en_curso`, por si el proceso no llega a
/// `finish`. Devuelve el código y el texto (`None` si ya se escribió el final).
fn write_report(timed_out: bool, running: Option<&str>) -> Option<(i32, String)> {
    let _writing = WRITING.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if FINISHED.load(Ordering::SeqCst) {
        return None;
    }
    let checks = RESULTS.lock().map(|results| results.clone()).unwrap_or_default();
    let failed = checks.iter().any(|check| check["estado"] == "falla");
    let code = if timed_out { 2 } else { i32::from(failed) };
    let path = output_path();
    let mut report = json!({
        "modo": "arranque",
        "version": env!("ATIC_VERSION"),
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "exe": std::env::current_exe().map(|exe| exe.display().to_string()).unwrap_or_default(),
        "ok": code == 0 && running.is_none(),
        "codigo": if running.is_none() { json!(code) } else { Value::Null },
        "plazo_vencido": timed_out,
        "ms": elapsed_ms(),
        "salida": path.display().to_string(),
        "comprobaciones": checks,
    });
    if let Some(name) = running {
        report["en_curso"] = json!(name);
    } else {
        FINISHED.store(true, Ordering::SeqCst);
    }
    let text = serde_json::to_string_pretty(&report).unwrap_or_default();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Err(error) = std::fs::write(&path, &text) {
        eprintln!("autoprueba: no se pudo escribir {}: {error}", path.display());
    }
    Some((code, text))
}

/// Escribe el JSON y termina el proceso con su código.
fn finish(timed_out: bool) -> ! {
    // Si el otro hilo ya escribió el final, este solo espera a que salga.
    let Some((code, text)) = write_report(timed_out, None) else {
        loop {
            std::thread::park();
        }
    };
    println!("{text}");
    let _ = std::io::stdout().flush();
    std::process::exit(code);
}

pub fn run(mode: &str) -> ! {
    STARTED.get_or_init(Instant::now);
    if mode != "arranque" {
        record("modo", "falla", 0, &format!("PILL_SELFTEST={mode} no existe; el único modo es «arranque»"));
        finish(false);
    }
    // Nunca se queda pegada: pasado el plazo escribe lo que alcanzó y sale.
    let deadline = std::env::var("PILL_SELFTEST_PLAZO").ok().and_then(|s| s.parse().ok()).unwrap_or(120u64);
    let _ = std::thread::Builder::new().name("autoprueba-plazo".into()).spawn(move || {
        std::thread::sleep(Duration::from_secs(deadline));
        record("plazo", "falla", elapsed_ms(), &format!("no terminó en {deadline} s"));
        finish(true);
    });

    check("config", config);
    check("i18n", i18n);
    check("novedades", whats_new);
    native_checks();
    check("sidecar", sidecar);
    check("mcp", mcp);
    window_check();
}

/// Las carpetas de Atic y `config.json`, solo leyendo: si está roto, la pill
/// arrancaría con los valores por defecto sin decir nada.
fn config() -> Result<Step, String> {
    let dirs = atic_core::AppDirs::new().map_err(|error| format!("sin carpeta de datos: {error}"))?;
    let path = dirs.config_path();
    let detail = match std::fs::read_to_string(&path) {
        Ok(text) => {
            serde_json::from_str::<Value>(&text).map_err(|error| format!("{} no es JSON: {error}", path.display()))?;
            "config.json se lee".to_string()
        }
        Err(_) => "sin config.json: valores por defecto".to_string(),
    };
    let config = atic_core::Config::load(&path);
    serde_json::to_string(&config).map_err(|error| format!("la configuración no se serializa: {error}"))?;
    Ok(Step::Done(detail))
}

/// Los textos en los dos idiomas y los íconos que van dentro del ejecutable.
fn i18n() -> Result<Step, String> {
    const KEY: &str = "pill.agents.openCode";
    crate::i18n::set_language("es");
    let es = crate::i18n::t(KEY);
    crate::i18n::set_language("en");
    let en = crate::i18n::t(KEY);
    crate::i18n::init();
    if es == KEY || en == KEY || es == en {
        return Err(format!("las tablas no traducen {KEY}: «{es}» / «{en}»"));
    }
    let icon = crate::Assets.load("icons/clipboard.svg").ok().flatten();
    if icon.is_none_or(|bytes| bytes.is_empty()) {
        return Err("falta el ícono icons/clipboard.svg".into());
    }
    Ok(Step::Done(format!("idioma de la UI: {}", if crate::i18n::english() { "en" } else { "es" })))
}

/// Cada versión publicada lleva sus Novedades (`whats_new.rs`).
fn whats_new() -> Result<Step, String> {
    let version = env!("ATIC_VERSION");
    let log = crate::whats_new::changelog();
    if log.versions.iter().any(|release| release.version == version) {
        Ok(Step::Done(format!("{} versiones; está la {version}", log.versions.len())))
    } else {
        Err(format!("falta la entrada de {version} en assets/changelog.json"))
    }
}

#[cfg(target_os = "macos")]
fn native_checks() {
    for (name, run) in crate::mac_smoke::CHECKS {
        check(&format!("nativo: {name}"), || run().map(Step::Done));
    }
    // El test lo exige; aquí solo avisa: un runner sin sesión gráfica puede
    // no tener ninguna ventana abierta y no es un error de Atic.
    check("nativo: apps abiertas", || {
        let open = crate::running::scan().windows.len();
        Ok(if open == 0 { Step::Warn("no hay apps abiertas".into()) } else { Step::Done(format!("{open} abiertas")) })
    });
}

/// El par de `mac_smoke.rs` en Windows: las APIs nativas que no necesitan
/// ventana ni permisos.
#[cfg(windows)]
fn native_checks() {
    check("nativo: apps e íconos", || {
        let windir = std::env::var_os("SystemRoot").map_or_else(|| PathBuf::from(r"C:\Windows"), PathBuf::from);
        if crate::app_icon::icon_for(&windir.join("explorer.exe")).is_none() {
            return Err("sin el ícono de explorer.exe".into());
        }
        let _ = crate::app_icon::icon_for_aumid("Microsoft.WindowsCalculator_8wekyb3d8bbwe!App");
        let running = crate::running::scan();
        Ok(Step::Done(format!("{} apps con ventanas", running.windows.len())))
    });
    check("nativo: apariencia", || {
        let light = crate::theme::windows_light();
        let _ = crate::code::style::system_reduced_motion();
        let _ = crate::glass::backdrop_available();
        let _ = crate::agents::agent_in_front(0);
        Ok(Step::Done(format!("tema {}", if light { "claro" } else { "oscuro" })))
    });
    check("nativo: sistema", || {
        let backend = crate::system::os::Backend::start();
        let snapshot = futures::executor::block_on(backend.read(crate::system::os::Want { audio: true, procs: true, ..Default::default() }))
            .map_err(|_| "la lectura del sistema no respondió".to_string())?;
        if !snapshot.procs.is_some_and(|p| p.ram_total > 0) {
            return Err("sin la memoria total".into());
        }
        Ok(Step::Done(format!("{} pantallas", snapshot.displays.len())))
    });
    check("nativo: privacidad", || {
        let privacy = crate::privacy::Privacy::start();
        std::thread::sleep(Duration::from_millis(1500));
        Ok(Step::Done(format!("{} usos ahora", privacy.uses().len())))
    });
    // Windows.Media.Ocr usa el idioma del perfil: un equipo sin uno con OCR
    // (un runner de CI, por ejemplo) no lo tiene, y no es un error de Atic.
    check("nativo: ocr", || {
        let (w, h) = (120u32, 40u32);
        let mut bgra = vec![255u8; (w * h * 4) as usize];
        for y in 12..28 {
            for x in 10..110 {
                let i = ((y * w + x) * 4) as usize;
                bgra[i..i + 3].copy_from_slice(&[0, 0, 0]);
            }
        }
        Ok(match crate::ocr::recognize(w, h, &bgra) {
            Ok(_) => Step::Done("Windows.Media.Ocr responde".into()),
            Err(error) => Step::Warn(format!("Windows.Media.Ocr no responde: {error}")),
        })
    });
}

#[cfg(not(any(windows, target_os = "macos")))]
fn native_checks() {}

/// Lanza un proceso por stdio sin consola (en Windows, la pill no tiene una
/// y cada hijo abriría la suya).
fn spawn_piped(program: &Path, args: &[&str]) -> Result<Child, String> {
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .current_dir(std::env::temp_dir());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command.spawn().map_err(|error| format!("no arrancó {}: {error}", program.display()))
}

/// Manda `lines` y espera la respuesta JSON con ese `id`, hasta `timeout`.
fn ask(child: &mut Child, lines: &[Value], id: u64, timeout: Duration) -> Result<Value, String> {
    let stdout = child.stdout.take().ok_or("sin stdout")?;
    let (tx, rx) = mpsc::channel();
    // El lector termina solo cuando el proceso muere y se cierra su salida.
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if let Ok(value) = serde_json::from_str::<Value>(&line) {
                if tx.send(value).is_err() {
                    break;
                }
            }
        }
    });
    let stdin = child.stdin.as_mut().ok_or("sin stdin")?;
    for line in lines {
        writeln!(stdin, "{line}").and_then(|()| stdin.flush()).map_err(|error| format!("no se pudo escribir: {error}"))?;
    }
    let until = Instant::now() + timeout;
    loop {
        let left = until.saturating_duration_since(Instant::now());
        match rx.recv_timeout(left) {
            Ok(value) if value["id"] == id => return Ok(value),
            Ok(_) => {}
            Err(mpsc::RecvTimeoutError::Timeout) => return Err(format!("no respondió en {} s", timeout.as_secs())),
            Err(mpsc::RecvTimeoutError::Disconnected) => return Err("se cerró sin responder".into()),
        }
    }
}

fn stop(mut child: Child) {
    let _ = child.kill();
    let _ = child.wait();
}

/// El agente de Atic Code: uno propio en modo stdio (no el proceso aparte de
/// la pill, ni su `code-agent.json`) que conteste `ping`.
fn sidecar() -> Result<Step, String> {
    let Some(script) = crate::code::sidecar::script_path() else {
        return Ok(Step::Skipped("no hay agent.mjs".into()));
    };
    // Exigida (el Verify del release), tiene que ser la del paquete: la pill
    // también acepta el agent.mjs del repo en la ruta de compilación, que en
    // un runner puede existir y tapar que falta en el paquete.
    if listed("PILL_SELFTEST_EXIGE", "sidecar") && crate::code::sidecar::bundled_script_path().as_ref() != Some(&script) {
        return Err(format!("usa {} y no un agent.mjs empaquetado junto al ejecutable", script.display()));
    }
    let Some(node) = crate::code::sidecar::node_path() else {
        return Ok(Step::Skipped("no hay Node".into()));
    };
    let mut child = spawn_piped(&node, &[&script.to_string_lossy()])?;
    let reply = ask(&mut child, &[json!({ "id": 1, "method": "ping", "params": {} })], 1, Duration::from_secs(30));
    stop(child);
    let reply = reply?;
    if reply["result"]["pong"] != true {
        return Err(format!("respuesta inesperada: {reply}"));
    }
    Ok(Step::Done(script.display().to_string()))
}

/// `atic-mcp` con las herramientas del editor (no necesita el hub): que se
/// presente y liste sus herramientas.
fn mcp() -> Result<Step, String> {
    let Some(binary) = crate::agents_settings::mcp_binary() else {
        return Ok(Step::Skipped("no hay atic-mcp".into()));
    };
    let mut child = spawn_piped(&binary, &["--editor"])?;
    let hello = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": { "name": "atic-selftest", "version": env!("ATIC_VERSION") },
        },
    });
    let started = json!({ "jsonrpc": "2.0", "method": "notifications/initialized" });
    let list = json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" });
    let reply = ask(&mut child, &[hello, started, list], 2, Duration::from_secs(20));
    stop(child);
    let tools = reply?["result"]["tools"].as_array().map_or(0, Vec::len);
    if tools == 0 {
        return Err("no lista herramientas".into());
    }
    Ok(Step::Done(format!("{tools} herramientas; {}", binary.display())))
}

/// La ventana de prueba: dibuja un texto y un ícono con el overlay puesto.
struct Probe {
    _overlay: Option<crate::win::Overlay>,
}

impl Render for Probe {
    fn render(&mut self, window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        if FRAMES.fetch_add(1, Ordering::SeqCst) < 3 {
            window.request_animation_frame();
        }
        div()
            .size_full()
            .flex()
            .gap_2()
            .items_center()
            .child(gpui::svg().path("icons/clipboard.svg").size(px(16.)))
            .child(SharedString::from(crate::i18n::t("pill.agents.openCode")))
    }
}

/// Abre una ventana chica y transparente como la de la pill (sin foco, en la
/// esquina) y espera a que dibuje. Termina el proceso.
fn window_check() -> ! {
    if listed("PILL_SELFTEST_OMITE", "ventana") {
        record("ventana", "omitido", 0, "PILL_SELFTEST_OMITE");
        finish(false);
    }
    // gpui puede terminar el proceso sin volver (sin Metal, por ejemplo).
    write_report(false, Some("ventana"));
    Application::new().with_assets(crate::Assets).run(|cx: &mut App| {
        let started = Instant::now();
        let Some(display) = cx.primary_display() else {
            let status = if listed("PILL_SELFTEST_EXIGE", "ventana") { "falla" } else { "omitido" };
            record("ventana", status, 0, "no hay pantalla");
            finish(false);
        };
        let origin = display.bounds().origin;
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::new(point(origin.x + px(8.), origin.y + px(8.)), size(px(220.), px(40.))))),
            titlebar: None,
            focus: false,
            show: true,
            kind: WindowKind::PopUp,
            is_movable: false,
            is_resizable: false,
            is_minimizable: false,
            display_id: Some(display.id()),
            window_background: WindowBackgroundAppearance::Transparent,
            ..Default::default()
        };
        let mut overlay_ok = false;
        let opened = cx.open_window(options, |window, cx| {
            let overlay = crate::win::Overlay::attach(window);
            if let Some(overlay) = &overlay {
                overlay.exclude_from_capture(true);
                overlay_ok = true;
            }
            cx.new(|_| Probe { _overlay: overlay })
        });
        if let Err(error) = opened {
            record("ventana", "falla", started.elapsed().as_millis(), &format!("no se abrió: {error:#}"));
            finish(false);
        }
        record(
            "overlay",
            if overlay_ok { "ok" } else { "falla" },
            started.elapsed().as_millis(),
            if overlay_ok { "Overlay::attach" } else { "Overlay::attach no encontró la ventana nativa" },
        );
        write_report(false, Some("ventana"));
        cx.spawn(async move |cx| {
            let until = Instant::now() + Duration::from_secs(15);
            while FRAMES.load(Ordering::SeqCst) < 2 && Instant::now() < until {
                cx.background_executor().timer(Duration::from_millis(50)).await;
            }
            let frames = FRAMES.load(Ordering::SeqCst);
            let ms = started.elapsed().as_millis();
            if frames >= 2 {
                record("ventana", "ok", ms, &format!("{frames} cuadros"));
            } else {
                record("ventana", "falla", ms, &format!("{frames} cuadros en 15 s"));
            }
            finish(false);
        })
        .detach();
    });
    // En macOS `run` no vuelve; en Windows vuelve si la app se cierra antes.
    record("ventana", "falla", elapsed_ms(), "la app se cerró antes de dibujar");
    finish(false);
}
