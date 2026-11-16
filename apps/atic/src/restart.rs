//! Reiniciar Atic para que un cambio tome efecto, y volver a donde se estaba.
//!
//! Hay cosas que Atic solo lee al arrancar (el PATH con un Node o un CLI de agente
//! recién instalado, un permiso de macOS). En vez de pedirle al usuario que la
//! cierre y la abra, `ask` muestra un aviso con «Reiniciar ahora» y `now` lo hace
//! de una: guarda adónde volver en `pill/resume.json`, lanza una instancia nueva y
//! cierra esta por el camino normal. El agente de Atic Code (`agent.mjs --daemon`)
//! no se toca: sus conversaciones siguen y la ventana nueva las retoma.
//!
//! - macOS: con la `.app`, `platform::reopen_after_exit` espera a que este proceso
//!   termine y la vuelve a abrir (`open`). Sin `.app` (un binario suelto), se
//!   lanza el mismo ejecutable con `--restarted`.
//! - Windows: el mismo `.exe`, con sus argumentos (`--native`) y `--restarted`.
//!
//! Con `--restarted`, la nueva espera unos segundos a que la vieja suelte el
//! candado de instancia única (`single_instance.rs`) en vez de cerrarse.
//!
//! Al arrancar, `resume_if_pending` abre lo guardado si tiene menos de 2 minutos
//! (si el reinicio se demoró más, ya no se sabe si el usuario sigue en eso).

use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use gpui::{
    div, prelude::*, px, size, App, Bounds, Context, FontWeight, SharedString, Window, WindowBounds,
    WindowKind, WindowOptions,
};
use gpui_m3::theme::type_scale::{self, TypeScaleExt};
use serde::{Deserialize, Serialize};

use crate::i18n::{t, tf};
use crate::kit::chrome;
use crate::settings::{hsla, MUTED, TEXT};

/// El argumento con que se lanza la instancia nueva.
const RESTARTED_ARG: &str = "--restarted";
/// Hasta cuándo la nueva espera el candado de la vieja.
const LOCK_WAIT: Duration = Duration::from_secs(15);
/// Lo guardado más viejo que esto no se abre.
const RESUME_MAX_AGE: Duration = Duration::from_secs(120);

const TOP_H: f32 = 40.0;
const WINDOW_W: f32 = 440.0;
const WINDOW_H: f32 = 280.0;

/// Adónde volver después de reiniciar.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Resume {
    /// La Configuración de Atic Code en una pestaña: `general`, `claude`,
    /// `appearance`, `shortcuts`, `agents` o `phone` (`CodeView::open_settings_at`).
    CodeSettings { tab: String },
    /// Atic Code con el detalle de un agente (`claude`, `codex`, `opencode`…) en
    /// Configuración → Agentes. Con `agent` vacío o desconocido, solo Atic Code
    /// (donde está la tarjeta «Preparar Atic Code»).
    CodeAgentSetup { agent: String },
    /// Ajustes en una sección: `general`, `appearance`, `pill`, `clipboard`,
    /// `dictation`, `meetings`, `captures`, `launcher`, `shortcuts`, `agents`,
    /// `phone`, `computers`, `permissions`, `guides` o `about`.
    Settings { section: String },
    /// macOS: la pantalla de permisos (el paso de Permisos de la bienvenida).
    Permissions,
    /// Solo la ventana de Atic Code.
    Code,
    /// Nada que reabrir.
    None,
}

#[derive(Serialize, Deserialize)]
struct Saved {
    resume: Resume,
    /// Segundos desde 1970.
    at: u64,
}

fn file() -> Option<PathBuf> {
    crate::paths::file("resume.json")
}

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or_default()
}

/// Deja anotado adónde volver en el próximo arranque (sin reiniciar). Sirve
/// también a quien reinicia por su cuenta, como el instalador de una
/// actualización.
pub fn remember(resume: Resume) {
    let Some(path) = file() else {
        return;
    };
    if resume == Resume::None {
        let _ = std::fs::remove_file(path);
        return;
    }
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let saved = Saved { resume, at: now_secs() };
    match serde_json::to_vec(&saved) {
        Ok(bytes) => {
            if let Err(error) = std::fs::write(&path, bytes) {
                tracing::warn!(%error, "reinicio: no se pudo guardar adónde volver");
            }
        }
        Err(error) => tracing::warn!(%error, "reinicio: no se pudo armar adónde volver"),
    }
}

/// Lo guardado, si es reciente. Lo borra siempre: se abre una sola vez.
fn take() -> Option<Resume> {
    let path = file()?;
    let raw = std::fs::read(&path).ok()?;
    let _ = std::fs::remove_file(&path);
    let saved: Saved = serde_json::from_slice(&raw).ok()?;
    fresh(saved.at, now_secs()).then_some(saved.resume)
}

fn fresh(at: u64, now: u64) -> bool {
    now >= at && now - at < RESUME_MAX_AGE.as_secs()
}

// --- Reiniciar -----------------------------------------------------------------------

/// Muestra un aviso «Atic necesita reiniciarse para <why>» con «Reiniciar ahora» y
/// «Después»; al aceptar, guarda `resume` y reinicia. `why` es una frase ya
/// traducida que completa «…para»: «usar el Node recién instalado» / «use the
/// Node you just installed».
pub fn ask(why: &str, resume: Resume, cx: &mut App) {
    let why = SharedString::from(why.trim().trim_end_matches('.').to_string());
    // Si ya hay un aviso abierto, se reemplaza su motivo: vale el último pedido.
    if let Some(handle) = cx.windows().into_iter().find_map(|w| w.downcast::<RestartPrompt>()) {
        let updated = handle.update(cx, |prompt, window, cx| {
            prompt.why = why.clone();
            prompt.resume = resume.clone();
            crate::win::bring_to_front(window);
            cx.notify();
        });
        if updated.is_ok() {
            return;
        }
    }
    let options = WindowOptions {
        titlebar: Some(chrome::titlebar(t("pill.restart.windowTitle"), TOP_H)),
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(None, size(px(WINDOW_W), px(WINDOW_H)), cx))),
        focus: crate::win::FOCUS_ON_OPEN,
        show: true,
        kind: WindowKind::Normal,
        is_resizable: false,
        is_minimizable: false,
        ..Default::default()
    };
    let opened = cx.open_window(options, |window, cx| {
        chrome::setup(window);
        cx.new(|_| RestartPrompt { why, resume, backdrop: None })
    });
    match opened {
        Ok(handle) => {
            let _ = handle.update(cx, |_, window, _| crate::win::bring_to_front(window));
        }
        Err(error) => tracing::warn!(%error, "reinicio: no se pudo abrir el aviso"),
    }
}

/// Reinicia ya, guardando `resume`. Si no se puede lanzar la instancia nueva,
/// Atic sigue abierta y avisa que hay que reabrirla a mano.
pub fn now(resume: Resume, cx: &mut App) {
    remember(resume.clone());
    match relaunch() {
        Ok(()) => {
            tracing::info!(?resume, "reinicio: Atic se cierra y vuelve a abrirse");
            cx.quit();
        }
        Err(error) => {
            tracing::warn!(%error, "reinicio: no se pudo lanzar la instancia nueva");
            remember(Resume::None);
            crate::platform::tray_icon::notify(t("pill.restart.failedTitle"), t("pill.restart.failedBody"), true, None);
        }
    }
}

/// Deja lista la instancia nueva. Quien llama cierra esta después.
fn relaunch() -> std::io::Result<()> {
    #[cfg(target_os = "macos")]
    if let Some(app) = crate::platform::bundle_path() {
        return crate::platform::reopen_after_exit(&app);
    }
    spawn_same_exe()
}

/// El mismo ejecutable, con sus argumentos y `--restarted` (espera el candado).
fn spawn_same_exe() -> std::io::Result<()> {
    let exe = std::env::current_exe()?;
    let mut command = std::process::Command::new(exe);
    command.args(std::env::args_os().skip(1).filter(|arg| !matches!(arg.to_str(), Some(RESTARTED_ARG | "--from-atic"))));
    command.arg(RESTARTED_ARG);
    // La Atic que vuelve no hereda los `PILL_*` de pruebas (como `freeze_watch`).
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("PILL_") {
            command.env_remove(key);
        }
    }
    command.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        command.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }
    command.spawn().map(drop)
}

/// Si esta instancia viene de un reinicio (`--restarted`), espera a que la
/// anterior suelte el candado. `None` si no viene de un reinicio o si la
/// anterior no terminó a tiempo.
pub fn acquire_after_restart() -> Option<crate::single_instance::Guard> {
    if !std::env::args_os().any(|arg| arg.to_str() == Some(RESTARTED_ARG)) {
        return None;
    }
    let deadline = std::time::Instant::now() + LOCK_WAIT;
    while std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(200));
        if let Some(guard) = crate::single_instance::acquire() {
            return Some(guard);
        }
    }
    tracing::warn!("reinicio: la instancia anterior no soltó el candado a tiempo");
    None
}

// --- Volver --------------------------------------------------------------------------

/// Al arrancar: si la vez anterior se reinició para un cambio, abre lo que
/// estaba a la vista.
pub fn resume_if_pending(cx: &mut App) {
    let Some(resume) = take() else {
        return;
    };
    tracing::info!(?resume, "reinicio: se vuelve a donde se estaba");
    // Un momento: la pill recién se abrió y Atic Code es pesado de armar.
    cx.spawn(async move |cx| {
        cx.background_executor().timer(Duration::from_millis(500)).await;
        let _ = cx.update(|cx| open(resume, cx));
    })
    .detach();
}

fn open(resume: Resume, cx: &mut App) {
    match resume {
        Resume::Settings { section } => match section_of(&section) {
            Some(section) => crate::settings::open_at(section, cx),
            None => crate::settings::open(cx),
        },
        Resume::Permissions => {
            #[cfg(target_os = "macos")]
            crate::permission_setup::open(cx);
            #[cfg(not(target_os = "macos"))]
            crate::settings::open(cx);
        }
        Resume::Code => {
            open_code(cx);
        }
        Resume::CodeSettings { tab } => {
            if let Some(handle) = open_code(cx) {
                let _ = handle.update(cx, |view, _, cx| view.open_settings_at(&tab, None, cx));
            }
        }
        Resume::CodeAgentSetup { agent } => {
            let known = crate::agents::AGENTS.iter().any(|a| a.cli == agent);
            if let Some(handle) = open_code(cx).filter(|_| known) {
                let _ = handle.update(cx, |view, _, cx| view.open_settings_at("agents", Some(&agent), cx));
            }
        }
        Resume::None => {}
    }
}

/// Atic Code al frente, por la pill (que lleva la cuenta de su ventana) o sola
/// si no hay pill (`CODE_ALONE`).
fn open_code(cx: &mut App) -> Option<gpui::WindowHandle<crate::code::CodeView>> {
    let find = |cx: &App| cx.windows().into_iter().find_map(|w| w.downcast::<crate::code::CodeView>());
    crate::Pill::open_code_from_app(cx);
    if let Some(handle) = find(cx) {
        return Some(handle);
    }
    match crate::code::open_window(cx) {
        Ok(handle) => Some(handle),
        Err(error) => {
            tracing::warn!(%error, "reinicio: no se pudo abrir Atic Code");
            None
        }
    }
}

fn section_of(key: &str) -> Option<crate::settings::Section> {
    use crate::settings::Section;
    Some(match key {
        "general" => Section::General,
        "appearance" => Section::Appearance,
        "pill" => Section::Pill,
        "clipboard" => Section::Clipboard,
        "dictation" => Section::Dictation,
        "meetings" => Section::Meetings,
        "captures" => Section::Captures,
        "launcher" => Section::Launcher,
        "shortcuts" => Section::Shortcuts,
        "agents" => Section::Agents,
        "phone" => Section::Phone,
        "computers" => Section::Computers,
        "permissions" => Section::Permissions,
        "guides" => Section::Guides,
        "about" => Section::About,
        _ => return None,
    })
}

// --- El aviso ------------------------------------------------------------------------

pub struct RestartPrompt {
    why: SharedString,
    resume: Resume,
    backdrop: Option<bool>,
}

impl Render for RestartPrompt {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::theme::sync_backdrop(window, &mut self.backdrop);
        let m3_scope = crate::theme::scope_m3(cx);
        let expressive = crate::theme::expressive().is_some();
        let primary = gpui_m3::Theme::of(cx).primary;

        let top = div()
            .h(px(TOP_H))
            .flex_none()
            .pl(px(16.))
            .flex()
            .items_center()
            .child(chrome::lights())
            .child(chrome::drag(TOP_H))
            .child(chrome::controls(false, TOP_H));

        let body = div()
            .flex_1()
            .min_h_0()
            .px(px(28.))
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(gpui_m3::Icon::new("refresh").size(px(26.)).color(primary))
            .child(
                div()
                    .text_size(px(22.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .when(expressive, |el| el.type_style(type_scale::HEADLINE_SMALL))
                    .child(t("pill.restart.title")),
            )
            .child(
                div()
                    .text_size(px(14.))
                    .when(expressive, |el| el.type_style(type_scale::BODY_LARGE))
                    .child(tf("pill.restart.reason", &[("why", &self.why)])),
            )
            .child(
                div()
                    .text_size(px(13.))
                    .text_color(hsla(MUTED))
                    .when(expressive, |el| el.type_style(type_scale::BODY_MEDIUM))
                    .child(t("pill.restart.back")),
            );

        let resume = self.resume.clone();
        let actions = div()
            .flex_none()
            .px(px(24.))
            .py(px(18.))
            .flex()
            .justify_end()
            .gap(px(8.))
            .child(
                gpui_m3::Button::new("restart-later", t("pill.restart.later"))
                    .text()
                    .on_click(|_, window, _| window.remove_window()),
            )
            .child(
                gpui_m3::Button::new("restart-now", t("pill.restart.now"))
                    .filled()
                    .autofocus(true)
                    .on_click(move |_, _, cx| now(resume.clone(), cx)),
            );

        div()
            .size_full()
            .flex()
            .flex_col()
            .map(|el| crate::theme::window_frame(el, window))
            .bg(crate::theme::window_bg())
            .font_family(crate::theme::font())
            .text_color(hsla(TEXT))
            .child(top)
            .child(body)
            .child(actions)
            .children(m3_scope)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lo_guardado_vale_dos_minutos() {
        assert!(fresh(1_000, 1_000));
        assert!(fresh(1_000, 1_119));
        assert!(!fresh(1_000, 1_120));
        // Un reloj que se fue para atrás no reabre nada.
        assert!(!fresh(1_000, 999));
    }

    #[test]
    fn adonde_volver_se_lee_como_se_escribe() {
        for resume in [
            Resume::CodeSettings { tab: "agents".into() },
            Resume::CodeAgentSetup { agent: "codex".into() },
            Resume::Settings { section: "about".into() },
            Resume::Permissions,
            Resume::Code,
            Resume::None,
        ] {
            let raw = serde_json::to_string(&Saved { resume: resume.clone(), at: 7 }).unwrap();
            let back: Saved = serde_json::from_str(&raw).unwrap();
            assert_eq!(back.resume, resume);
            assert_eq!(back.at, 7);
        }
    }

    #[test]
    fn las_secciones_de_ajustes_se_reconocen() {
        assert!(section_of("about").is_some());
        assert!(section_of("permissions").is_some());
        assert!(section_of("nada").is_none());
    }
}
