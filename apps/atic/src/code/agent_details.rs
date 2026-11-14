//! El detalle de un agente en Ajustes → Agentes: su cuenta (con o sin sesión, y cómo), el
//! uso de su plan, las suscripciones y claves que tiene (OpenCode) y si sus consolas tienen
//! las herramientas de Atic. Iniciar o cerrar sesión y agregar una clave corren el comando
//! del propio CLI en la terminal integrada, a la vista (ahí se escribe la clave o se abre el
//! navegador); Atic nunca guarda ni ve las claves.
//!
//! Lo que se lee tarda (corre CLIs, consulta cupos): se pide al abrir el detalle, en otro
//! hilo, y se guarda aquí hasta «Volver a revisar».

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use gpui::{div, prelude::*, px, AnyElement, ClickEvent, Context, FontWeight, SharedString};

use super::style::t;
use super::subscriptions::{self, Status, Sub};
use super::CodeView;
use crate::i18n::{t as tr, tf};
use crate::quota::AgentQuota;

/// La cuenta de un agente según su CLI.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Account {
    pub signed_in: bool,
    /// Cómo («Suscripción de Claude», «ChatGPT», «API key»…), si se sabe.
    pub how: Option<String>,
}

/// Lo que se sabe de un agente para su detalle.
#[derive(Clone, Default)]
pub(super) struct Info {
    /// `None`: el CLI no tiene cómo preguntarlo (se ve al iniciar sesión).
    pub account: Option<Account>,
    pub quota: Option<AgentQuota>,
    pub subs: Vec<Sub>,
    /// Las herramientas de Atic (`atic-mcp`) en las consolas de este CLI; `None` si no se puede.
    pub mcp: Option<bool>,
}

#[derive(Default)]
struct State {
    open: Option<&'static str>,
    info: HashMap<&'static str, Info>,
    loading: HashSet<&'static str>,
    /// Instalando o quitando las herramientas de Atic.
    mcp_busy: HashSet<&'static str>,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

fn with<R>(f: impl FnOnce(&mut State) -> R) -> R {
    let mut guard = STATE.lock().unwrap();
    f(guard.get_or_insert_with(State::default))
}

/// Cómo se inicia y se cierra sesión en cada CLI (en la terminal integrada). Sin comando
/// para salir, el de cerrar sesión va vacío.
pub(super) fn auth_commands(cli: &str) -> Option<(&'static str, &'static str)> {
    Some(match cli {
        "claude" => ("claude auth login", "claude auth logout"),
        "codex" => ("codex login", "codex logout"),
        "opencode" => ("opencode auth login", "opencode auth logout"),
        "cursor-agent" => ("cursor-agent login", "cursor-agent logout"),
        "grok" => ("grok login", "grok logout"),
        // Antigravity no tiene `login` ni `logout`: al abrir `agy` sin sesión pide entrar con Google.
        "agy" => ("agy", ""),
        _ => return None,
    })
}

/// Las herramientas de Atic se pueden poner en las consolas de estos CLI.
fn mcp_supported(cli: &str) -> bool {
    matches!(cli, "claude" | "codex" | "opencode" | "cursor-agent" | "grok" | "agy")
}

/// Corre un CLI sin ventana y devuelve lo que escribió (stdout y stderr), o `None` si no
/// está o tardó más de `secs`.
fn run_cli(cli: &str, args: &[&str], secs: u64) -> Option<String> {
    let (program, prefix) = atic_agents::exe::launcher(cli)?;
    let mut command = std::process::Command::new(program);
    command.args(prefix).args(args).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = command.spawn().ok()?;
    let deadline = Instant::now() + Duration::from_secs(secs);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(50)),
            _ => {
                let _ = child.kill();
                return None;
            }
        }
    }
    let mut out = String::new();
    std::io::Read::read_to_string(&mut child.stdout.take()?, &mut out).ok()?;
    let mut err = String::new();
    if let Some(mut stderr) = child.stderr.take() {
        let _ = std::io::Read::read_to_string(&mut stderr, &mut err);
    }
    Some(format!("{out}\n{err}"))
}

/// `claude auth status` (JSON: `loggedIn`, `authMethod`, a veces `email`).
pub(super) fn parse_claude_status(text: &str) -> Option<Account> {
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    let v: serde_json::Value = serde_json::from_str(&text[start..=end]).ok()?;
    let signed_in = v.get("loggedIn")?.as_bool()?;
    let method = v.get("authMethod").and_then(serde_json::Value::as_str).unwrap_or_default();
    let how = match method {
        "" => None,
        "claude.ai" | "oauth" => Some(tr("pill.code.agentDetails.claudeSub").to_string()),
        m if m.contains("api") || m.contains("console") => Some(tr("pill.code.agentDetails.apiKey").to_string()),
        m => Some(m.to_string()),
    };
    let email = v.get("email").and_then(serde_json::Value::as_str).filter(|e| !e.is_empty());
    let how = match (how, email) {
        (Some(how), Some(email)) => Some(format!("{how} · {email}")),
        (how, email) => how.or(email.map(str::to_string)),
    };
    Some(Account { signed_in, how })
}

/// `codex login status` («Logged in using ChatGPT», «Logged in using an API key», «Not logged in»).
pub(super) fn parse_codex_status(text: &str) -> Option<Account> {
    let line = text.lines().map(str::trim).find(|l| !l.is_empty())?;
    let lower = line.to_lowercase();
    if lower.contains("not logged in") {
        return Some(Account { signed_in: false, how: None });
    }
    if !lower.contains("logged in") {
        return None;
    }
    let how = if lower.contains("api key") { tr("pill.code.agentDetails.apiKey").to_string() } else if lower.contains("chatgpt") { "ChatGPT".to_string() } else { line.to_string() };
    Some(Account { signed_in: true, how: Some(how) })
}

/// Lee todo lo del detalle de un agente (en otro hilo: corre CLIs y consulta cupos).
fn load(cli: &'static str) -> Info {
    let account = match cli {
        "claude" => run_cli("claude", &["auth", "status"], 15).as_deref().and_then(parse_claude_status),
        "codex" => run_cli("codex", &["login", "status"], 15).as_deref().and_then(parse_codex_status),
        _ => None,
    };
    let subs = if cli == "opencode" { subscriptions::opencode() } else { Vec::new() };
    // OpenCode: conectado si tiene alguna credencial.
    let account = account.or_else(|| (cli == "opencode").then(|| Account { signed_in: !subs.is_empty(), how: None }));
    let quota = crate::quota::fetch_overview(false).agents.into_iter().find(|q| q.agent == cli);
    // Antigravity: si su cupo responde, la sesión de Google está abierta.
    let account = account.or_else(|| (cli == "agy").then_some(quota.as_ref()).flatten().filter(|q| q.error.is_none() && !q.windows.is_empty()).map(|_| Account { signed_in: true, how: Some("Google".into()) }));
    let mcp = mcp_supported(cli).then(|| atic_agents::mcp_install::instalado(cli).ok()).flatten();
    Info { account, quota, subs, mcp }
}

impl CodeView {
    /// Abre o cierra el detalle de un agente; al abrirlo, lee lo que falte.
    pub(super) fn toggle_agent_details(&mut self, cli: &'static str, cx: &mut Context<Self>) {
        let open = with(|s| {
            s.open = if s.open == Some(cli) { None } else { Some(cli) };
            s.open == Some(cli) && !s.info.contains_key(cli)
        });
        if open {
            self.refresh_agent_details(cli, cx);
        }
        cx.notify();
    }

    pub(super) fn refresh_agent_details(&mut self, cli: &'static str, cx: &mut Context<Self>) {
        if !with(|s| s.loading.insert(cli)) {
            return;
        }
        cx.notify();
        cx.spawn(async move |this, cx| {
            let info = cx.background_spawn(async move { load(cli) }).await;
            with(|s| {
                s.loading.remove(cli);
                s.info.insert(cli, info);
            });
            let _ = this.update(cx, |_, cx| cx.notify());
        })
        .detach();
    }

    /// Pone o quita las herramientas de Atic en las consolas de un CLI.
    fn set_agent_mcp(&mut self, cli: &'static str, on: bool, cx: &mut Context<Self>) {
        let Some(exe) = crate::agents_settings::mcp_binary() else {
            self.show_toast(tr("pill.code.agentDetails.noMcp"), cx);
            return;
        };
        if !with(|s| s.mcp_busy.insert(cli)) {
            return;
        }
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    if on { atic_agents::mcp_install::instalar(cli, &exe) } else { atic_agents::mcp_install::quitar(cli) }
                        .and_then(|_| atic_agents::mcp_install::instalado(cli))
                })
                .await;
            let _ = this.update(cx, |view, cx| {
                with(|s| {
                    s.mcp_busy.remove(cli);
                    if let (Ok(now), Some(info)) = (&result, s.info.get_mut(cli)) {
                        info.mcp = Some(*now);
                    }
                });
                if let Err(error) = result {
                    view.show_toast(SharedString::from(error), cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// ¿Está abierto el detalle de este agente?
    pub(super) fn agent_details_open(&self, cli: &str) -> bool {
        with(|s| s.open == Some(cli))
    }

    /// El detalle de un agente, bajo su fila.
    pub(super) fn agent_details(&self, cli: &'static str, name: &'static str, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let t = t();
        let (info, loading, mcp_busy) = with(|s| (s.info.get(cli).cloned(), s.loading.contains(cli), s.mcp_busy.contains(cli)));
        let section = |title: SharedString| div().pt(px(6.)).text_size(px(11.5)).font_weight(FontWeight::SEMIBOLD).text_color(t.muted).child(title);
        let line = |text: SharedString| div().text_size(px(13.)).text_color(t.text).child(text);
        let faint = |text: SharedString| div().text_size(px(12.)).text_color(t.muted).child(text);
        let mut body = div().flex().flex_col().gap(px(6.)).px(px(14.)).pb(px(12.)).pt(px(2.));

        let Some(info) = info else {
            return body.child(faint(tr("pill.code.agentDetails.loading").into())).into_any_element();
        };

        // Cuenta.
        body = body.child(section(tr("pill.code.agentDetails.account").into()));
        let account_text: SharedString = match &info.account {
            Some(Account { signed_in: true, how: Some(how) }) => tf("pill.code.agentDetails.signedInHow", &[("how", how)]).into(),
            Some(Account { signed_in: true, how: None }) => tr("pill.code.agentDetails.signedIn").into(),
            Some(Account { signed_in: false, .. }) => tr("pill.code.agentDetails.signedOut").into(),
            None => match info.quota.as_ref().and_then(|q| q.plan.clone()) {
                Some(plan) => tf("pill.code.agentDetails.plan", &[("plan", &plan)]).into(),
                None => tr("pill.code.agentDetails.unknown").into(),
            },
        };
        let mut account_row = div().flex().items_center().gap(px(8.)).child(div().flex_1().min_w(px(0.)).child(line(account_text)));
        if let Some((login, logout)) = auth_commands(cli) {
            let signed_in = info.account.as_ref().is_some_and(|a| a.signed_in);
            if !signed_in || cli == "opencode" {
                account_row = account_row.child(
                    gpui_m3::Button::new(("agent-login", index), if cli == "opencode" { tr("pill.code.agentDetails.addProvider") } else { tr("pill.code.agentDetails.login") })
                        .tonal()
                        .size(gpui_m3::ButtonSize::Small)
                        .tooltip(login)
                        .on_click(cx.listener(move |view, _: &ClickEvent, window, cx| {
                            with(|s| s.info.remove(cli));
                            let login = super::agents_tab::with_resolved_cli(cli, login);
                            view.run_agent_command(&login, &tf("pill.code.agentDetails.loginTitle", &[("name", &name)]), window, cx);
                        })),
                );
            }
            if signed_in && !logout.is_empty() {
                account_row = account_row.child(
                    gpui_m3::Button::new(("agent-logout", index), tr("pill.code.agentDetails.logout"))
                        .text()
                        .size(gpui_m3::ButtonSize::Small)
                        .tooltip(logout)
                        .on_click(cx.listener(move |view, _: &ClickEvent, window, cx| {
                            with(|s| s.info.remove(cli));
                            let logout = super::agents_tab::with_resolved_cli(cli, logout);
                            view.run_agent_command(&logout, &tf("pill.code.agentDetails.logoutTitle", &[("name", &name)]), window, cx);
                        })),
                );
            }
        }
        body = body.child(account_row);

        // Suscripciones y claves (OpenCode): con cuál responde cada modelo.
        if cli == "opencode" {
            body = body.child(section(tr("pill.code.agentDetails.subscriptions").into()));
            if info.subs.is_empty() {
                body = body.child(faint(tr("pill.code.agentDetails.noSubscriptions").into()));
            }
            for sub in &info.subs {
                let (label, color) = match sub.status {
                    Status::Active => (tr("pill.code.agentDetails.active"), t.text),
                    Status::Exhausted => (tr("pill.code.agentDetails.exhausted"), t.muted),
                    Status::NoBalance => (tr("pill.code.agentDetails.noBalance"), t.muted),
                    Status::Invalid => (tr("pill.code.agentDetails.invalid"), t.muted),
                };
                body = body.child(
                    div()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(8.))
                                .child(div().flex_1().min_w(px(0.)).text_size(px(13.)).text_color(t.text).child(super::acp::provider_name(&sub.provider)))
                                .child(div().flex_none().text_size(px(12.)).text_color(color).child(label)),
                        )
                        .when_some(sub.detail.clone(), |el, detail| el.child(faint(detail.into()))),
                );
            }
            body = body.child(faint(tr("pill.code.agentDetails.subscriptionsHint").into()));
        }

        // Uso del plan.
        if let Some(quota) = info.quota.as_ref() {
            if !quota.windows.is_empty() {
                body = body.child(section(tr("pill.code.agentDetails.usage").into()));
                for window in &quota.windows {
                    let used = window.used_percent.clamp(0., 100.);
                    let reset = window.resets_at.map(|ms| {
                        let left = (ms - chrono::Utc::now().timestamp_millis()).max(0) / 60_000;
                        if left >= 60 * 24 { format!("{} d", left / (60 * 24)) } else if left >= 60 { format!("{} h", left / 60) } else { format!("{left} min") }
                    });
                    let text = match reset {
                        Some(reset) => tf("pill.code.agentDetails.windowReset", &[("kind", &window.kind), ("used", &format!("{used:.0}")), ("reset", &reset)]),
                        None => tf("pill.code.agentDetails.window", &[("kind", &window.kind), ("used", &format!("{used:.0}"))]),
                    };
                    body = body.child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(3.))
                            .child(faint(text.into()))
                            .child(
                                div().h(px(4.)).w_full().rounded_full().bg(t.hover).child(
                                    div().h_full().rounded_full().w(gpui::relative((used / 100.) as f32)).bg(if used >= 90. { t.bad } else { t.accent }),
                                ),
                            ),
                    );
                }
            } else if let Some(error) = &quota.error {
                body = body.child(section(tr("pill.code.agentDetails.usage").into())).child(faint(error.clone().into()));
            }
        }

        // Las herramientas de Atic en las consolas de este CLI.
        if let Some(on) = info.mcp {
            body = body.child(section(tr("pill.code.agentDetails.atic").into())).child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(div().flex_1().min_w(px(0.)).child(faint(tr("pill.code.agentDetails.aticDesc").into())))
                    .child(
                        gpui_m3::Switch::new(("agent-mcp", index), on).disabled(mcp_busy).on_toggle({
                            let view = cx.weak_entity();
                            move |on, _, cx| {
                                let _ = view.update(cx, |view, cx| view.set_agent_mcp(cli, on, cx));
                            }
                        }),
                    ),
            );
        }

        if loading {
            body = body.child(faint(tr("pill.code.agentDetails.loading").into()));
        }
        body.into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lee_la_cuenta_de_claude() {
        let out = r#"{ "loggedIn": true, "authMethod": "claude.ai", "apiProvider": "firstParty" }"#;
        let account = parse_claude_status(out).unwrap();
        assert!(account.signed_in);
        assert!(account.how.is_some());
        assert_eq!(parse_claude_status(r#"{"loggedIn": false}"#).unwrap().signed_in, false);
        assert_eq!(parse_claude_status("roto"), None);
    }

    #[test]
    fn lee_la_cuenta_de_codex() {
        assert_eq!(parse_codex_status("Logged in using ChatGPT").unwrap().how.as_deref(), Some("ChatGPT"));
        assert!(!parse_codex_status("Not logged in").unwrap().signed_in);
        assert_eq!(parse_codex_status("algo raro"), None);
    }

    #[test]
    fn cada_cli_con_cuenta_sabe_entrar_y_salir() {
        for cli in ["claude", "codex", "opencode", "cursor-agent", "grok"] {
            let (login, logout) = auth_commands(cli).unwrap();
            assert!(login.contains("login") && logout.contains("logout"), "{cli}");
        }
        assert_eq!(auth_commands("agy"), Some(("agy", "")));
        assert_eq!(auth_commands("gemini"), None);
    }
}

/// «Volver a revisar»: lo leído se olvida y se vuelve a pedir al abrir cada detalle.
pub(super) fn forget() {
    with(|s| {
        s.info.clear();
        s.open = None;
    });
}
