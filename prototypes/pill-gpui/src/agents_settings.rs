//! La sección Agentes de la ventana de Ajustes: conectar cada CLI con Atic
//! (el servidor MCP `atic`), para que las consolas que abres tú puedan
//! delegar en otros agentes. Instalar y quitar se le pide al propio CLI
//! (`atic_agents::mcp_install`), en segundo plano: tarda lo que tarda el CLI.

use std::path::PathBuf;

use atic_agents::mcp_install;
use gpui::{div, prelude::*, px, App, ClickEvent, Context, Entity, SharedString, Window};

use crate::agents::AGENTS;
use crate::i18n::t;
use crate::meetings::settings::{card, heading, row, switch};
use crate::settings::{hsla, MUTED};

/// Cómo está un CLI.
#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Checking,
    Missing,
    Off,
    On,
    Busy,
}

pub struct AgentsPane {
    states: Vec<State>,
    error: Option<SharedString>,
}

/// `atic-mcp.exe` junto al exe de la pill (así lo deja el instalador) o en la
/// instalación por usuario.
fn sidecar() -> Option<PathBuf> {
    let beside = std::env::current_exe().ok()?.parent()?.join("atic-mcp.exe");
    if beside.is_file() {
        return Some(beside);
    }
    let installed = PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join("Atic").join("atic-mcp.exe");
    installed.is_file().then_some(installed)
}

pub fn agents_pane(cx: &mut App) -> Entity<AgentsPane> {
    cx.new(|cx| {
        let mut pane = AgentsPane { states: vec![State::Checking; AGENTS.len()], error: None };
        for index in 0..AGENTS.len() {
            pane.refresh(index, cx);
        }
        pane
    })
}

impl AgentsPane {
    /// Mira en segundo plano si el CLI está y si ya tiene el servidor.
    fn refresh(&mut self, index: usize, cx: &mut Context<Self>) {
        let cli = AGENTS[index].cli;
        cx.spawn(async move |pane, cx| {
            let state = cx
                .background_spawn(async move {
                    if atic_agents::exe::resolve(cli).is_none() {
                        return State::Missing;
                    }
                    match mcp_install::instalado(cli) {
                        Ok(true) => State::On,
                        _ => State::Off,
                    }
                })
                .await;
            let _ = pane.update(cx, |pane, cx| {
                pane.states[index] = state;
                cx.notify();
            });
        })
        .detach();
    }

    fn toggle(&mut self, index: usize, cx: &mut Context<Self>) {
        let on = self.states[index] == State::On;
        let Some(sidecar) = sidecar() else {
            self.error = Some(t("settings.agents.hubPathMissing").into());
            cx.notify();
            return;
        };
        self.states[index] = State::Busy;
        self.error = None;
        cx.notify();
        let cli = AGENTS[index].cli;
        cx.spawn(async move |pane, cx| {
            let result = cx
                .background_spawn(async move {
                    if on {
                        mcp_install::quitar(cli)
                    } else {
                        mcp_install::instalar(cli, &sidecar)
                    }
                })
                .await;
            let _ = pane.update(cx, |pane, cx| {
                if let Err(error) = result {
                    pane.error = Some(error.into());
                }
                pane.refresh(index, cx);
            });
        })
        .detach();
    }
}

impl Render for AgentsPane {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut list = card();
        for (index, agent) in AGENTS.iter().enumerate() {
            let state = self.states[index];
            let hint: SharedString = match state {
                State::Missing => t("settings.agents.mcpMissing").into(),
                State::Checking | State::Busy => "…".into(),
                _ => SharedString::default(),
            };
            let control = match state {
                State::On | State::Off => switch(
                    agent.cli,
                    state == State::On,
                    cx.listener(move |pane, _: &ClickEvent, _, cx| pane.toggle(index, cx)),
                )
                .into_any_element(),
                _ => div().into_any_element(),
            };
            list = list.child(row(agent.name, hint, control));
        }
        div()
            .flex()
            .flex_col()
            .child(heading(t("settings.agents.aticMcpTitle")))
            .child(
                div()
                    .px(px(4.))
                    .pb(px(8.))
                    .text_size(px(12.))
                    .text_color(hsla(MUTED))
                    .child(t("settings.agents.aticMcpHint")),
            )
            .child(list)
            .children(self.error.clone().map(|error| {
                div().pt(px(8.)).px(px(4.)).text_size(px(12.)).text_color(hsla(crate::settings::AMBER)).child(error)
            }))
    }
}
