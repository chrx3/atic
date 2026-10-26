//! «Preparar Atic Code»: lo que el chat necesita en la máquina (Node para el agente,
//! Claude Code y su sesión). Si falta algo, el inicio muestra una tarjeta con un
//! botón por paso; cada uno corre en la terminal integrada, a la vista, y la tarjeta
//! se revisa sola hasta que todo queda listo.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use gpui::{div, prelude::*, px, AnyElement, ClickEvent, Context, FontWeight, SharedString};

use super::style::t;
use super::{sidecar, CodeView};
use crate::i18n::t as tr;

/// Cada cuánto se revisa mientras un paso corre en la terminal, y hasta cuándo.
const POLL: Duration = Duration::from_secs(3);
const POLL_FOR: Duration = Duration::from_secs(15 * 60);

/// Lo que hay en la máquina.
#[derive(Clone, Copy, Default, PartialEq)]
pub(super) struct Setup {
    pub node: bool,
    pub claude: bool,
    pub signed_in: bool,
}

impl Setup {
    pub fn ready(&self) -> bool {
        self.node && self.claude && self.signed_in
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Step {
    Node,
    Claude,
    SignIn,
}

/// Revisa todo, con la ruta de Claude Code que encontró; lanza procesos, así que va
/// en otro hilo.
pub(super) fn check() -> (Setup, Option<PathBuf>) {
    let claude = sidecar::claude_path();
    let found = Setup {
        node: sidecar::node_path().is_some(),
        signed_in: claude.as_deref().is_some_and(signed_in),
        claude: claude.is_some(),
    };
    (found, claude)
}

/// `claude auth status` responde un JSON con `loggedIn`. Se corta a los 10 s.
fn signed_in(claude: &Path) -> bool {
    let mut command = std::process::Command::new(claude);
    command
        .args(["auth", "status"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let Ok(mut child) = command.spawn() else {
        return false;
    };
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(50)),
            _ => {
                let _ = child.kill();
                return false;
            }
        }
    }
    let mut out = String::new();
    let Some(mut stdout) = child.stdout.take() else {
        return false;
    };
    if std::io::Read::read_to_string(&mut stdout, &mut out).is_err() {
        return false;
    }
    logged_in(&out)
}

fn logged_in(status: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(status)
        .ok()
        .and_then(|value| value.get("loggedIn").and_then(serde_json::Value::as_bool))
        .unwrap_or(false)
}

/// Lo que se corre en la terminal para un paso (PowerShell en Windows, el shell del
/// usuario en macOS). `None`: no hay comando y se abre la descarga en el navegador.
fn command(step: Step, claude: Option<&Path>) -> Option<String> {
    match step {
        Step::Node if cfg!(windows) => {
            Some("winget install -e --id OpenJS.NodeJS.LTS --accept-source-agreements --accept-package-agreements".into())
        }
        Step::Node => atic_agents::exe::resolve("brew").map(|brew| format!("\"{}\" install node", brew.display())),
        Step::Claude => crate::agents::AGENTS.iter().find(|agent| agent.cli == "claude").map(|agent| agent.install.to_string()),
        Step::SignIn => Some(match claude {
            Some(path) if cfg!(windows) => format!("& \"{}\" auth login", path.display()),
            Some(path) => format!("\"{}\" auth login", path.display()),
            None => "claude auth login".into(),
        }),
    }
}

const NODE_DOWNLOAD: &str = "https://nodejs.org/en/download";

impl CodeView {
    /// Revisa en segundo plano. Con `poll`, sigue revisando mientras falte algo (un
    /// paso corriendo en la terminal), hasta `POLL_FOR`.
    pub(super) fn load_setup(&mut self, poll: bool, cx: &mut Context<Self>) {
        if poll {
            self.setup_poll_until = Some(Instant::now() + POLL_FOR);
        }
        if self.setup_checking {
            return;
        }
        self.setup_checking = true;
        cx.spawn(async move |this, cx| loop {
            let (found, claude) = cx.background_spawn(async { check() }).await;
            let again = this
                .update(cx, |view, cx| {
                    view.setup_checking = false;
                    // Claude Code recién instalado: el agente que haya quedado vivo sin él
                    // (si se intentó chatear antes) se cierra y la próxima petición lo
                    // levanta con la ruta nueva.
                    if found.claude && view.setup.is_some_and(|setup| !setup.claude) {
                        view.sidecar.shutdown();
                    }
                    if claude.is_some() {
                        view.claude_path = claude;
                    }
                    if view.setup != Some(found) {
                        view.setup = Some(found);
                        cx.notify();
                    }
                    let polling = view.setup_poll_until.is_some_and(|until| Instant::now() < until);
                    let again = polling && !found.ready();
                    if again {
                        view.setup_checking = true;
                    } else {
                        view.setup_poll_until = None;
                    }
                    again
                })
                .unwrap_or(false);
            if !again {
                break;
            }
            cx.background_executor().timer(POLL).await;
        })
        .detach();
    }

    fn run_setup_step(&mut self, step: Step, window: &mut gpui::Window, cx: &mut Context<Self>) {
        let name = match step {
            Step::Node => tr("pill.code.setup.nodeTitle"),
            Step::Claude => tr("pill.code.setup.claudeTitle"),
            Step::SignIn => tr("pill.code.setup.signInTitle"),
        };
        match command(step, self.claude_path.as_deref()) {
            Some(command) => {
                let home = crate::paths::home().map(std::ffi::OsString::from).map(PathBuf::from);
                self.run_in_terminal(command, name, home, window, cx);
            }
            None => {
                if let Err(error) = crate::launcher::shell_open(NODE_DOWNLOAD) {
                    self.show_toast(error, cx);
                }
            }
        }
        self.load_setup(true, cx);
    }

    /// La tarjeta del inicio cuando falta algo; `None` si todo está listo o aún no se
    /// revisa (así no parpadea para quien ya tiene todo).
    pub(super) fn setup_card(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let setup = self.setup.filter(|setup| !setup.ready())?;
        let t = t();
        let steps = [
            (Step::Node, setup.node, "pill.code.setup.nodeTitle", "pill.code.setup.nodeDesc", "pill.code.setup.install"),
            (Step::Claude, setup.claude, "pill.code.setup.claudeTitle", "pill.code.setup.claudeDesc", "pill.code.setup.install"),
            (Step::SignIn, setup.signed_in, "pill.code.setup.signInTitle", "pill.code.setup.signInDesc", "pill.code.setup.signIn"),
        ];
        let mut list = div().flex().flex_col().gap(px(6.));
        for (index, (step, done, title, desc, action)) in steps.into_iter().enumerate() {
            // Sin Claude Code no hay con qué iniciar sesión.
            let blocked = step == Step::SignIn && !setup.claude;
            let mark = if done {
                div()
                    .size(px(22.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .bg(t.accent)
                    .child(gpui_m3::Icon::new("check").size(px(14.)).color(t.on_accent))
            } else {
                div()
                    .size(px(22.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .border_2()
                    .border_color(t.muted.opacity(0.5))
                    .text_size(px(11.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(t.muted)
                    .child((index + 1).to_string())
            };
            let mut row = div()
                .id(("setup-step", index))
                .flex()
                .items_center()
                .gap(px(12.))
                .px(px(14.))
                .py(px(10.))
                .rounded(px(t.r_ctl.min(14.)))
                .bg(t.control)
                .child(mark)
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .child(div().text_size(px(13.)).font_weight(FontWeight::SEMIBOLD).text_color(t.text).child(tr(title)))
                        .child(div().text_size(px(12.)).text_color(t.muted).child(tr(desc))),
                );
            if !done {
                row = row.child(
                    div().flex_none().child(
                        gpui_m3::Button::new(("setup-run", index), tr(action))
                            .tonal()
                            .size(gpui_m3::ButtonSize::Small)
                            .disabled(blocked)
                            .on_click(cx.listener(move |view, _: &ClickEvent, window, cx| view.run_setup_step(step, window, cx))),
                    ),
                );
            }
            list = list.child(row);
        }
        let checking: SharedString = if self.setup_checking { tr("pill.code.setup.checking").into() } else { tr("pill.code.setup.recheck").into() };
        Some(
            div()
                .id("code-setup")
                .w_full()
                .flex()
                .flex_col()
                .gap(px(10.))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .flex()
                                .flex_col()
                                .gap(px(2.))
                                .child(div().text_size(px(15.)).font_weight(FontWeight::SEMIBOLD).text_color(t.text).child(tr("pill.code.setup.title")))
                                .child(div().text_size(px(12.5)).text_color(t.muted).child(tr("pill.code.setup.intro"))),
                        )
                        .child(
                            div().flex_none().child(
                                gpui_m3::Button::new("setup-recheck", checking)
                                    .text()
                                    .size(gpui_m3::ButtonSize::Small)
                                    .icon("refresh")
                                    .disabled(self.setup_checking)
                                    .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.load_setup(false, cx))),
                            ),
                        ),
                )
                .child(list)
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_sesion_sale_del_json_de_auth_status() {
        assert!(logged_in(r#"{ "loggedIn": true, "authMethod": "claude.ai" }"#));
        assert!(!logged_in(r#"{ "loggedIn": false }"#));
        assert!(!logged_in("Not logged in"));
        assert!(!logged_in(""));
    }

    #[test]
    fn iniciar_sesion_usa_la_ruta_de_claude() {
        let path = Path::new("/x/claude");
        let login = command(Step::SignIn, Some(path)).unwrap();
        assert!(login.contains("/x/claude") && login.ends_with("auth login"));
        assert_eq!(command(Step::SignIn, None).as_deref(), Some("claude auth login"));
    }
}
