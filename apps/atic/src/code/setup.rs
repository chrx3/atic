//! «Preparar Atic Code»: lo que el chat necesita en la máquina (Node para el agente,
//! Claude Code y su sesión). Si falta algo, el inicio muestra una tarjeta con los pasos
//! del agente elegido (`agent_setup.rs`): uno por uno o «Instalar lo que falta», en la
//! terminal integrada, a la vista, y la tarjeta se revisa sola hasta que todo queda listo.
//!
//! Claude Code no es obligatorio: con otro agente instalado (Codex, OpenCode, Cursor,
//! Grok) se chatea con ese, y la tarjeta solo pide Node (que corre a todos los agentes).
//! La primera revisión también elige con qué agente parte una conversación nueva: el
//! último que se usó o, si Claude no está listo, uno que sí esté.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use gpui::{div, prelude::*, px, AnyElement, ClickEvent, Context, FontWeight, SharedString};

use super::style::t;
use super::{acp, agent_setup, sidecar, CodeView};
use crate::i18n::{t as tr, tf};

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

/// Revisa todo, con la ruta de Claude Code que encontró y los agentes ACP instalados;
/// lanza procesos, así que va en otro hilo.
pub(super) fn check() -> (Setup, Option<PathBuf>, Vec<&'static str>) {
    let claude = sidecar::claude_path();
    let found = Setup {
        node: sidecar::node_path().is_some(),
        signed_in: claude.as_deref().is_some_and(signed_in),
        claude: claude.is_some(),
    };
    let others = acp::AGENTS.iter().filter(|agent| crate::agents::on_path(agent.cli)).map(|agent| agent.cli).collect();
    (found, claude, others)
}

/// Con qué agente parte una conversación nueva (`None` es Claude): el último elegido si
/// sigue a mano; si no, o si era Claude y no está listo, el primer agente ACP visible.
fn start_agent(last: Option<&str>, claude_ready: bool, visible: &[&'static str]) -> Option<&'static str> {
    let last = match last {
        Some("claude") if claude_ready => return None,
        Some(cli) => visible.iter().copied().find(|c| *c == cli),
        None => None,
    };
    last.or_else(|| if claude_ready { None } else { visible.first().copied() })
}

/// `claude auth status` responde un JSON con `loggedIn`. Se corta a los 10 s.
pub(super) fn signed_in(claude: &Path) -> bool {
    let mut command = std::process::Command::new(claude);
    atic_agents::exe::apply_fresh_path(&mut command);
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
        // Los pasos de Claude y, si se eligió otro, los de ese agente (`agent_setup.rs`).
        let acp = self.current_agent();
        cx.spawn(async move |this, cx| loop {
            let (found, claude, others, reports) = cx
                .background_spawn(async move {
                    let (found, claude, others) = check();
                    let mut reports = vec![("claude", agent_setup::inspect("claude", false))];
                    if let Some(cli) = acp {
                        reports.push((cli, agent_setup::inspect(cli, true)));
                    }
                    (found, claude, others, reports)
                })
                .await;
            let again = this
                .update(cx, |view, cx| {
                    view.setup_checking = false;
                    for (cli, mut report) in reports {
                        if cli == "claude" {
                            report.signed_in = found.claude.then_some(found.signed_in);
                        }
                        view.store_setup_report(cli, report);
                    }
                    if view.setup.is_none() {
                        view.pick_start_agent(found, others, cx);
                    }
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

    /// La primera vez: lo último elegido en cada agente ACP vuelve a sus borradores, y la
    /// conversación nueva parte con el agente que toca (`start_agent`).
    fn pick_start_agent(&mut self, found: Setup, others: Vec<&'static str>, cx: &mut Context<Self>) {
        for (cli, draft) in self.acp_drafts.iter_mut() {
            if let Some(wanted) = self.configs.acp_choices.get(*cli) {
                draft.apply_wanted(wanted);
            }
        }
        // La lista de instalados la llena también `CodeView::new`; lo que se vio aquí sirve
        // si esta revisión termina antes.
        for cli in others {
            if !self.acp_installed.contains(&cli) {
                self.acp_installed.push(cli);
            }
        }
        if self.next_agent.is_some() {
            return;
        }
        let claude_ready = found.claude && found.signed_in;
        if let Some(cli) = start_agent(self.configs.last_agent.as_deref(), claude_ready, &self.visible_acp()) {
            self.choose_agent(Some(cli), cx);
        }
    }

    /// La tarjeta del inicio cuando falta algo; `None` si todo está listo o aún no se
    /// revisa (así no parpadea para quien ya tiene todo). Un paso corriendo o que falló
    /// manda: se ven los pasos de su agente.
    pub(super) fn setup_card(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let pending = self.setup_run.as_ref().map(|run| run.agent).or_else(|| self.setup_failed.as_ref().map(|failed| failed.agent));
        let cli = match pending {
            Some(cli) => cli,
            None => {
                // Con otro agente elegido, Claude Code y su sesión no hacen falta: solo Node.
                let claude = self.current_agent().is_none();
                self.setup.filter(|setup| if claude { !setup.ready() } else { !setup.node })?;
                self.current_agent().unwrap_or("claude")
            }
        };
        let steps = self.agent_setup_steps(cli, cx)?;
        let t = t();
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
                                .child(div().text_size(px(12.5)).text_color(t.muted).child(tf("pill.code.agentSetup.intro", &[("name", &agent_setup::agent_name(cli))]))),
                        )
                        .child(
                            div().flex_none().child(
                                gpui_m3::Button::new("setup-recheck", checking)
                                    .text()
                                    .size(gpui_m3::ButtonSize::Small)
                                    .icon("refresh")
                                    .disabled(self.setup_checking)
                                    .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                                        if view.setup_run.is_none() {
                                            view.setup_failed = None;
                                        }
                                        view.load_setup(false, cx);
                                        view.refresh_agent_setup(cli, cx);
                                    })),
                            ),
                        ),
                )
                .child(steps)
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
    fn sin_claude_listo_parte_con_otro_agente() {
        let visible = ["opencode", "cursor-agent"];
        // Sin nada guardado: Claude si está listo; si no, el primero que haya.
        assert_eq!(start_agent(None, true, &visible), None);
        assert_eq!(start_agent(None, false, &visible), Some("opencode"));
        // El último elegido gana mientras siga a mano.
        assert_eq!(start_agent(Some("cursor-agent"), true, &visible), Some("cursor-agent"));
        assert_eq!(start_agent(Some("claude"), true, &visible), None);
        // Claude elegido pero sin instalar o sin sesión: otro que sí esté.
        assert_eq!(start_agent(Some("claude"), false, &visible), Some("opencode"));
        // Uno que ya no está (o se ocultó): Claude si se puede.
        assert_eq!(start_agent(Some("grok"), true, &visible), None);
        // Sin ningún otro, Claude aunque falte algo: la tarjeta dice qué.
        assert_eq!(start_agent(None, false, &[]), None);
    }
}
