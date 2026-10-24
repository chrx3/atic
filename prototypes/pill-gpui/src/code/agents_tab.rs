//! La pestaña «Agentes» de la configuración: los CLI de agentes que Atic Code sabe
//! usar, con su versión instalada, si se muestran en el chat y la terminal, y botones
//! para instalarlos o actualizarlos (corren en la terminal integrada, a la vista).

use std::path::PathBuf;
use std::time::{Duration, Instant};

use gpui::{div, prelude::*, px, svg, AnyElement, ClickEvent, Context, FontWeight, SharedString};

use super::style::t;
use crate::i18n::{t as tr, tf};
use super::{acp, CodeView};

/// Un agente de la lista: su CLI, nombre, logo, cómo se instala y si va también en el
/// chat (ACP) o solo en la terminal.
#[derive(Clone, Copy)]
pub(super) struct AgentRow {
    pub cli: &'static str,
    pub name: &'static str,
    pub logo: &'static str,
    pub install: &'static str,
}

/// Todos los agentes: el catálogo de la pill y Gemini (que solo va en la terminal).
pub(super) fn rows() -> Vec<AgentRow> {
    crate::agents::AGENTS
        .iter()
        .map(|agent| AgentRow { cli: agent.cli, name: agent.name, logo: agent.logo, install: agent.install })
        .chain(std::iter::once(AgentRow {
            cli: "gemini",
            name: "Gemini",
            logo: "icons/agents/gemini.svg",
            install: "npm install -g @google/gemini-cli",
        }))
        .collect()
}

/// La versión de un CLI (`--version`, primera línea), o `None` si no está. Se corta a
/// los 10 s: algún CLI podría quedarse esperando.
pub(super) fn version_of(cli: &str) -> Option<String> {
    let (program, prefix) = atic_agents::exe::launcher(cli)?;
    let mut command = std::process::Command::new(program);
    command.args(prefix).arg("--version").stdin(std::process::Stdio::null()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = command.spawn().ok()?;
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(50)),
            _ => {
                let _ = child.kill();
                return Some(tr("pill.code.agentsTab.installed").into());
            }
        }
    }
    let mut out = String::new();
    std::io::Read::read_to_string(&mut child.stdout.take()?, &mut out).ok()?;
    let line = out.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or(tr("pill.code.agentsTab.installed"));
    Some(short_version(line))
}

/// Lo legible de una línea de `--version` («grok 1.0.52 (0920c8) [stable]» → «1.0.52»).
fn short_version(line: &str) -> String {
    line.split_whitespace()
        .map(|word| word.trim_start_matches('v'))
        .find(|word| word.chars().next().is_some_and(|c| c.is_ascii_digit()) && word.contains('.'))
        .unwrap_or(line)
        .to_string()
}

impl CodeView {
    /// Mira qué agentes hay y su versión, en segundo plano (al abrir la pestaña y con
    /// «Volver a revisar»).
    pub(super) fn load_agent_versions(&mut self, cx: &mut Context<Self>) {
        if self.versions_loading {
            return;
        }
        self.versions_loading = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let found = cx
                .background_spawn(async move {
                    let handles: Vec<_> = rows().into_iter().map(|row| std::thread::spawn(move || (row.cli, version_of(row.cli)))).collect();
                    handles.into_iter().filter_map(|h| h.join().ok()).collect::<Vec<_>>()
                })
                .await;
            let _ = this.update(cx, |view, cx| {
                view.versions_loading = false;
                view.agent_versions = found.into_iter().collect();
                // Lo recién instalado aparece en el chat sin reabrir la ventana.
                view.acp_installed =
                    acp::AGENTS.iter().filter(|agent| view.agent_versions.get(agent.cli).is_some_and(Option::is_some)).map(|agent| agent.cli).collect();
                cx.notify();
            });
        })
        .detach();
    }

    /// Instala o actualiza un agente en la terminal integrada.
    fn run_agent_command(&mut self, command: &str, name: &str, window: &mut gpui::Window, cx: &mut Context<Self>) {
        let home = crate::paths::home().map(std::ffi::OsString::from).map(PathBuf::from);
        self.settings_open = false;
        self.run_in_terminal(command.to_string(), name, home, window, cx);
    }

    pub(super) fn agents_tab(&self, cx: &mut Context<Self>) -> AnyElement {
        let t = t();
        let head = div()
            .flex()
            .items_center()
            .gap(px(10.))
            .pb(px(10.))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .text_size(px(13.))
                    .text_color(t.muted)
                    .child(tr("pill.code.agentsTab.intro")),
            )
            .child(
                div().flex_none().child(gpui_m3::Button::new("agents-recheck", if self.versions_loading { tr("pill.code.agentsTab.checking") } else { tr("pill.code.agentsTab.recheck") })
                    .text()
                    .size(gpui_m3::ButtonSize::Small)
                    .icon("refresh")
                    .disabled(self.versions_loading)
                    .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.load_agent_versions(cx))),
            ));
        let mut list = div().flex().flex_col().gap(px(6.));
        for (index, row) in rows().into_iter().enumerate() {
            let version = self.agent_versions.get(row.cli).cloned();
            let installed = version.as_ref().is_some_and(Option::is_some);
            let checked = version.is_some();
            let chat = row.cli == "claude" || acp::agent(row.cli).is_some();
            let where_ = if chat { tr("pill.code.agentsTab.chatAndTerminal") } else { tr("pill.code.agentsTab.terminalOnly") };
            let status: SharedString = match &version {
                Some(Some(version)) => tf("pill.code.agentsTab.version", &[("where", &where_), ("version", version)]).into(),
                Some(None) => tf("pill.code.agentsTab.notInstalled", &[("where", &where_)]).into(),
                None => tf("pill.code.agentsTab.checkingStatus", &[("where", &where_)]).into(),
            };
            let hidden = self.configs.agent_hidden(row.cli);
            let mut actions = div().flex().items_center().gap(px(8.)).flex_none();
            if checked && !installed {
                let (command, name) = (row.install, row.name);
                actions = actions.child(
                    gpui_m3::Button::new(("agent-install", index), tr("pill.code.agentsTab.install"))
                        .tonal()
                        .size(gpui_m3::ButtonSize::Small)
                        .tooltip(row.install)
                        .on_click(cx.listener(move |view, _: &ClickEvent, window, cx| view.run_agent_command(command, &tf("pill.code.agentsTab.installTitle", &[("name", &name)]), window, cx))),
                );
            }
            if installed {
                let command = acp::update_command(row.cli).unwrap_or(row.install);
                let name = row.name;
                actions = actions.child(
                    gpui_m3::Button::new(("agent-update", index), tr("pill.code.agentsTab.update"))
                        .text()
                        .size(gpui_m3::ButtonSize::Small)
                        .tooltip(command)
                        .on_click(cx.listener(move |view, _: &ClickEvent, window, cx| view.run_agent_command(command, &tf("pill.code.agentsTab.updateTitle", &[("name", &name)]), window, cx))),
                );
                // Claude es el de siempre: no se oculta.
                if row.cli != "claude" {
                    let cli = row.cli;
                    actions = actions.child(
                        div()
                            .id(("agent-show", index))
                            .tooltip(crate::hover::tip(if hidden { tr("pill.code.agentsTab.hiddenTip") } else { tr("pill.code.agentsTab.shownTip") }))
                            .child(gpui_m3::Switch::new(("agent-switch", index), !hidden).on_toggle({
                                let view = cx.weak_entity();
                                move |on, _, cx| {
                                    let _ = view.update(cx, |view, cx| {
                                        view.configs.set_agent_hidden(cli, !on);
                                        cx.notify();
                                    });
                                }
                            })),
                    );
                }
            }
            list = list.child(
                div()
                    .id(("agent-row", index))
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .px(px(14.))
                    .py(px(10.))
                    .rounded(px(t.r_ctl.min(14.)))
                    .bg(t.control)
                    .when(hidden && installed, |el| el.opacity(0.6))
                    .child(svg().path(row.logo).size(px(22.)).flex_none().text_color(t.text))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .child(div().text_size(px(14.)).font_weight(FontWeight::MEDIUM).text_color(t.text).child(row.name))
                            .child(div().text_size(px(12.)).text_color(t.muted).truncate().child(status)),
                    )
                    .child(actions),
            );
        }
        div().flex().flex_col().child(head).child(list).into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_version_es_lo_legible() {
        assert_eq!(short_version("grok 1.0.52 (0920c8169e27) [stable]"), "1.0.52");
        assert_eq!(short_version("opencode v2.0.24"), "2.0.24");
        assert_eq!(short_version("2.1.299 (Claude Code)"), "2.1.299");
        assert_eq!(short_version("2026.09.28-64d2043"), "2026.09.28-64d2043");
    }

    #[test]
    fn la_lista_tiene_a_todos_y_a_gemini() {
        let rows = rows();
        assert!(rows.iter().any(|r| r.cli == "claude"));
        assert!(rows.iter().any(|r| r.cli == "gemini"));
        // Cada uno se sabe instalar.
        assert!(rows.iter().all(|r| !r.install.is_empty()));
    }
}
