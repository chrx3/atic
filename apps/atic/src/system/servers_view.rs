//! Los servidores locales en Rendimiento (`servers.rs`): una fila con cuántos hay levantados
//! y cuántos parecen olvidados, que al tocarla despliega uno por fila. Con el cursor, cada
//! uno ofrece abrirlo en el navegador o detenerlo (dos clics, como «Forzar» en Procesos).

use gpui::{div, prelude::*, px, svg, AnyElement, ClickEvent, Context, SharedString};

use super::os::Cmd;
use super::servers::Server;
use super::{detail, proc_action, row, Arm, SystemPanel, ROW_H};
use crate::hover::hover_fx;

/// Hace cuánto, en pocas letras: «25 min», «3 h», «2 d».
pub(super) fn age_short(secs: u64) -> String {
    if secs < 3600 {
        format!("{} min", (secs / 60).max(1))
    } else if secs < 86_400 {
        format!("{} h", secs / 3600)
    } else {
        format!("{} d", secs / 86_400)
    }
}

/// «:3000 · 3 h · sin conexiones».
fn facts(server: &Server) -> String {
    let ports = server.ports.iter().map(|p| format!(":{p}")).collect::<Vec<_>>().join(" ");
    let mut parts = vec![ports];
    if let Some(age) = server.age {
        parts.push(age_short(age));
    }
    parts.push(match server.connections {
        0 => crate::i18n::t("pill.system.servers.noConnections").to_string(),
        1 => crate::i18n::t("pill.system.servers.oneConnection").to_string(),
        n => crate::i18n::tf("pill.system.servers.connections", &[("n", &n)]),
    });
    parts.join(" · ")
}

impl SystemPanel {
    /// Cuántas filas suman los servidores en Rendimiento (para el alto del panel).
    pub(super) fn server_rows(&self) -> usize {
        let servers = &self.state.servers;
        if servers.is_empty() {
            0
        } else if self.servers_open {
            1 + servers.len()
        } else {
            1
        }
    }

    /// Hay alguno olvidado: la pestaña Rendimiento lo marca.
    pub(super) fn idle_servers(&self) -> usize {
        self.state.servers.iter().filter(|s| s.idle()).count()
    }

    pub(super) fn server_rows_ui(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let servers = self.state.servers.clone();
        if servers.is_empty() {
            return Vec::new();
        }
        let c = &self.colors;
        let (text, muted, amber, danger, ink) = (c.text, c.muted, c.amber, c.danger, c.ink);
        let idle = self.idle_servers();
        let open = self.servers_open;
        let mut rows = Vec::new();
        let summary = if idle > 0 {
            crate::i18n::tf("pill.system.servers.summaryIdle", &[("n", &servers.len()), ("idle", &idle)])
        } else {
            crate::i18n::tf("pill.system.servers.summary", &[("n", &servers.len())])
        };
        let toggle = cx.listener(|panel, _: &ClickEvent, _, cx| {
            panel.servers_open = !panel.servers_open;
            cx.notify();
        });
        let leading = svg().path("icons/square-terminal.svg").size(px(16.)).text_color(if idle > 0 { amber } else { muted }).into_any_element();
        let head = hover_fx(SharedString::from("system-servers-fx"), move |h| {
            let trailing = div()
                .flex_1()
                .min_w_0()
                .flex()
                .items_center()
                .justify_end()
                .gap(px(6.))
                .child(div().min_w_0().truncate().text_size(px(11.)).text_color(if idle > 0 { amber } else { muted }).child(summary.clone()))
                .child(svg().path(if open { "icons/chevron-up.svg" } else { "icons/chevron-down.svg" }).size(px(12.)).text_color(muted))
                .into_any_element();
            row(leading, crate::i18n::t("pill.system.servers.title").into(), trailing)
                .id("system-servers")
                .mx(px(-6.))
                .px(px(6.))
                .rounded(px(10.))
                .cursor_pointer()
                .bg(h.mix(gpui::white().opacity(0.0), gpui::white().opacity(0.05)))
                .on_click(toggle)
                .into_any_element()
        });
        rows.push(head.into_any_element());
        if !open {
            return rows;
        }
        for server in servers {
            let pid = server.pid;
            let armed = self.is_armed(&Arm::Server(pid));
            let name: SharedString = server.title().into();
            let line = facts(&server);
            let idle = server.idle();
            let url = server.ports.first().map(|port| format!("http://localhost:{port}"));
            let stop_name = server.title();
            let stop = cx.listener(move |panel, _: &ClickEvent, _, cx| {
                if panel.confirm(Arm::Server(pid), cx) {
                    panel.backend.send(Cmd::StopServer(pid, stop_name.clone()));
                    panel.refresh(cx);
                }
            });
            let open_url = url.clone();
            let open_it = move |_: &ClickEvent, _: &mut gpui::Window, cx: &mut gpui::App| {
                if let Some(url) = &open_url {
                    cx.open_url(url);
                }
            };
            let leading = div().size(px(6.)).rounded_full().bg(if idle { amber } else { gpui::rgb(super::DOWN_TINT).into() }).into_any_element();
            let id = SharedString::from(format!("server-{pid}"));
            let row_el = hover_fx(id.clone(), move |h| {
                let reveal = if armed { 1.0 } else { h.t };
                let trailing = div()
                    .flex_1()
                    .min_w_0()
                    .relative()
                    .h(px(ROW_H))
                    .flex()
                    .items_center()
                    .justify_end()
                    .child(div().min_w_0().truncate().opacity(1.0 - reveal).child(detail(line.clone(), if idle { amber } else { muted })))
                    .child(
                        div()
                            .absolute()
                            .right_0()
                            .top_0()
                            .bottom_0()
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .opacity(reveal)
                            .when(reveal < 0.05, |el| el.invisible())
                            .when(url.is_some(), |el| {
                                el.child(proc_action(format!("{id}-open").into(), crate::i18n::t("pill.system.servers.open"), text, ink, false, open_it))
                            })
                            .child(proc_action(
                                format!("{id}-stop").into(),
                                if armed { crate::i18n::t("pill.system.forceAsk") } else { crate::i18n::t("pill.system.servers.stop") },
                                danger,
                                ink,
                                armed,
                                stop,
                            )),
                    )
                    .into_any_element();
                row(leading, name.clone(), trailing).mx(px(-6.)).px(px(6.)).rounded(px(10.)).into_any_element()
            });
            rows.push(row_el.into_any_element());
        }
        rows
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_fila_dice_puerto_edad_y_conexiones() {
        let server = Server { pid: 1, ports: vec![3000, 3001], label: "Next.js".into(), project: Some("tienda".into()), age: Some(3 * 3600 + 5), connections: 0 };
        assert_eq!(facts(&server), ":3000 :3001 · 3 h · sin conexiones");
        assert_eq!(age_short(30), "1 min");
        assert_eq!(age_short(2 * 86_400), "2 d");
    }
}
