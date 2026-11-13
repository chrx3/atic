//! La sección Computadores de la ventana de Ajustes: prender el mouse y el
//! teclado compartidos, buscar y vincular otros computadores, de qué lado de
//! este está cada uno y el portapapeles. El servicio vive en `computers.rs`;
//! aquí se mira su estado cada segundo.

use std::time::Duration;

use gpui::{div, prelude::*, px, svg, App, ClickEvent, ClipboardItem, Context, Entity, Focusable, FontWeight, SharedString, Window};

use crate::computers::{self, Notice, PairWindow, PeerRow, Status};
use crate::hover::HoverExt;
use crate::i18n::{t, tf};
use crate::meetings::settings::{button, card, field, heading, row, switch};
use crate::settings::{hsla, AMBER, FAINT, FILL, INK, MUTED, SURFACE_ON, TEXT};
use crate::text_input::TextInput;
use atic_kvm::Side;

/// Los interruptores de «Usar este mouse y teclado en él», uno por equipo
/// (los ids de `switch` son estáticos).
const SHARE_IDS: [&str; 8] = ["kvm-share-0", "kvm-share-1", "kvm-share-2", "kvm-share-3", "kvm-share-4", "kvm-share-5", "kvm-share-6", "kvm-share-7"];

/// El tamaño de cada lado en el dibujo de dónde está el otro.
const SLOT_W: f32 = 84.;
const SLOT_H: f32 = 48.;
const SLOT_GAP: f32 = 6.;

pub struct ComputersPane {
    status: Status,
    /// El código de 6 dígitos que muestra el otro equipo.
    code: Entity<TextInput>,
    /// El id del otro, pegado a mano (cuando la búsqueda no lo encuentra).
    peer_id: Entity<TextInput>,
    /// Ya se pedía el código en el cuadro anterior (para darle el foco una vez).
    asked: bool,
    copied: bool,
    error: Option<SharedString>,
}

pub fn computers_pane(cx: &mut App) -> Entity<ComputersPane> {
    cx.new(|cx| {
        // El estado cambia solo (otro equipo se conecta o pide vincularse).
        cx.spawn(async move |pane, cx| loop {
            cx.background_executor().timer(Duration::from_secs(1)).await;
            let status = cx.background_spawn(async { computers::status() }).await;
            let alive = pane.update(cx, |pane: &mut ComputersPane, cx| {
                if pane.status != status {
                    pane.status = status;
                    cx.notify();
                }
            });
            if alive.is_err() {
                break;
            }
        })
        .detach();
        let code = cx.new(|cx| TextInput::new("000000", hsla(TEXT), hsla(FAINT), hsla(TEXT), cx));
        let peer_id = cx.new(|cx| TextInput::new(t("pill.kvm.idPlaceholder"), hsla(TEXT), hsla(FAINT), hsla(TEXT), cx));
        ComputersPane { status: computers::status(), code, peer_id, asked: false, copied: false, error: None }
    })
}

impl ComputersPane {
    fn refresh(&mut self, cx: &mut Context<Self>) {
        self.status = computers::status();
        cx.notify();
    }

    fn search(&mut self, cx: &mut Context<Self>) {
        self.error = None;
        cx.spawn(async move |pane, cx| {
            let searching = cx.background_spawn(async { computers::search() });
            // Que se vea «Buscando…» mientras escucha la red.
            cx.background_executor().timer(Duration::from_millis(50)).await;
            let _ = pane.update(cx, |pane, cx| pane.refresh(cx));
            searching.await;
            let _ = pane.update(cx, |pane, cx| pane.refresh(cx));
        })
        .detach();
    }

    fn pair(&mut self, id: String, name: Option<String>, cx: &mut Context<Self>) {
        self.error = computers::pair(&id, name).err().map(Into::into);
        self.code.update(cx, |input, cx| input.clear(cx));
        self.refresh(cx);
    }

    fn pair_by_id(&mut self, cx: &mut Context<Self>) {
        let id = self.peer_id.read(cx).text().trim().to_string();
        if id.is_empty() {
            return;
        }
        self.pair(id, None, cx);
        if self.error.is_none() {
            self.peer_id.update(cx, |input, cx| input.clear(cx));
        }
    }

    fn submit_code(&mut self, cx: &mut Context<Self>) {
        let code = self.code.read(cx).text().to_string();
        if code.chars().filter(char::is_ascii_digit).count() != 6 {
            self.error = Some(t("pill.kvm.codeIncomplete").into());
            cx.notify();
            return;
        }
        self.error = None;
        computers::submit_code(&code);
        self.code.update(cx, |input, cx| input.clear(cx));
        self.refresh(cx);
    }

    /// El código que hay que escribir en el otro equipo, grande.
    fn pair_code(&self, name: &str, code: &str) -> impl IntoElement {
        let spaced: String = code.chars().enumerate().flat_map(|(i, c)| (i == 3).then_some(' ').into_iter().chain([c])).collect();
        card().child(
            div()
                .flex()
                .flex_col()
                .items_center()
                .gap(px(8.))
                .px(px(16.))
                .py(px(20.))
                .child(div().text_size(px(13.)).font_weight(FontWeight::MEDIUM).child(tf("pill.kvm.codeTitle", &[("name", &name)])))
                .child(
                    div()
                        .text_size(px(36.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .font_family(crate::fonts::MONO)
                        .text_color(hsla(TEXT))
                        .child(spaced),
                )
                .child(div().text_size(px(12.)).text_color(hsla(MUTED)).child(t("pill.kvm.codeHint"))),
        )
    }

    /// Un equipo vinculado: su estado, si se usa este mouse y teclado en él y de qué lado está.
    fn peer(&self, index: usize, peer: &PeerRow, cx: &mut Context<Self>) -> impl IntoElement {
        let status = match (peer.connected, peer.mac) {
            (true, true) => t("pill.kvm.connectedMac"),
            (true, false) => t("pill.kvm.connectedPc"),
            (false, _) => t("pill.kvm.offline"),
        };
        let forget_id = peer.id.clone();
        let share_id = peer.id.clone();
        let sharing = peer.sharing;
        let share_hint = match (sharing, peer.side) {
            (true, Some(side)) => tf("pill.kvm.sharingHint", &[("side", &side_edge(side))]),
            _ => t("pill.kvm.shareHint").to_string(),
        };
        // El otro dice otra cosa y no se pone de acuerdo solo (versión anterior).
        let claim = peer.claim.map(|side| {
            let claim_id = peer.id.clone();
            row(
                tf(claim_key(side), &[("name", &peer.name)]),
                t("pill.kvm.claimHint"),
                button(
                    SharedString::from(format!("kvm-claim-{index}")),
                    t("pill.kvm.useClaim"),
                    cx.listener(move |pane, _: &ClickEvent, _, cx| {
                        computers::use_claim(&claim_id);
                        pane.refresh(cx);
                    }),
                ),
            )
        });
        card()
            .child(row(
                peer.name.clone(),
                status,
                button(
                    SharedString::from(format!("kvm-forget-{index}")),
                    t("pill.kvm.forget"),
                    cx.listener(move |pane, _: &ClickEvent, _, cx| {
                        computers::forget(&forget_id);
                        pane.refresh(cx);
                    }),
                ),
            ))
            // Lo principal: dónde está el otro respecto de esta pantalla.
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .px(px(16.))
                    .py(px(14.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .child(div().text_size(px(13.)).font_weight(FontWeight::MEDIUM).child(t("pill.kvm.where")))
                            .child(div().text_size(px(12.)).text_color(hsla(MUTED)).child(tf("pill.kvm.whereHint", &[("name", &peer.name)]))),
                    )
                    .child(side_picker(index, peer, cx)),
            )
            .children(claim)
            .child(row(
                t("pill.kvm.share"),
                share_hint,
                switch(
                    SHARE_IDS.get(index).copied().unwrap_or("kvm-share-x"),
                    sharing,
                    cx.listener(move |pane, _: &ClickEvent, _, cx| {
                        computers::set_sharing(&share_id, !sharing);
                        pane.refresh(cx);
                    }),
                ),
            ))
    }

    /// Si este equipo atiende a otro que pida vincularse, cuánto le queda y el
    /// botón para abrirlo o cerrarlo.
    fn pair_window(&self, window: PairWindow, cx: &mut Context<Self>) -> gpui::AnyElement {
        match window {
            PairWindow::Closed => row(
                t("pill.kvm.pairClosed"),
                t("pill.kvm.pairClosedHint"),
                button("kvm-pair-open".into(), t("pill.kvm.allowPair"), cx.listener(|pane, _: &ClickEvent, _, cx| {
                    computers::open_pairing();
                    pane.refresh(cx);
                })),
            )
            .into_any_element(),
            PairWindow::Open { minutes } => row(
                if minutes == 1 { t("pill.kvm.pairOpenOne").to_string() } else { tf("pill.kvm.pairOpen", &[("minutes", &minutes)]) },
                t("pill.kvm.pairOpenHint"),
                button("kvm-pair-close".into(), t("pill.kvm.pairClose"), cx.listener(|pane, _: &ClickEvent, _, cx| {
                    computers::close_pairing();
                    pane.refresh(cx);
                })),
            )
            .into_any_element(),
            PairWindow::Locked { minutes } => row(tf("pill.kvm.pairLocked", &[("minutes", &minutes)]), t("pill.kvm.pairLockedHint"), div()).into_any_element(),
        }
    }

    /// Buscar en la red, vincular por id y el id propio.
    fn add(&self, status: &Status, cx: &mut Context<Self>) -> impl IntoElement {
        let search_control = if status.searching {
            div().text_size(px(12.)).text_color(hsla(MUTED)).child(t("pill.kvm.searching")).into_any_element()
        } else {
            button("kvm-search".into(), t("pill.kvm.searchButton"), cx.listener(|pane, _: &ClickEvent, _, cx| pane.search(cx)))
        };
        let mut add = card()
            .child(self.pair_window(status.pair_window, cx))
            .child(row(t("pill.kvm.search"), t("pill.kvm.searchHint"), search_control));

        // Lo que encontró la búsqueda.
        match &status.nearby {
            Some(found) if found.is_empty() && !status.searching => {
                add = add.child(row(t("pill.kvm.noneFound"), t("pill.kvm.noneFoundHint"), div()));
            }
            Some(found) => {
                for (index, (id, name)) in found.iter().cloned().enumerate() {
                    let short: String = id.chars().take(10).collect();
                    let busy = status.pairing.is_some();
                    add = add.child(row(
                        name.clone(),
                        short,
                        if busy {
                            div().into_any_element()
                        } else {
                            button(
                                SharedString::from(format!("kvm-pair-{index}")),
                                t("pill.kvm.pair"),
                                cx.listener(move |pane, _: &ClickEvent, _, cx| pane.pair(id.clone(), Some(name.clone()), cx)),
                            )
                        },
                    ));
                }
            }
            None => {}
        }

        // Vinculando: primero se espera al otro; cuando muestra su código, se escribe aquí.
        if let Some((name, asking)) = &status.pairing {
            let cancel = button("kvm-pair-cancel".into(), t("pill.kvm.cancel"), cx.listener(|pane, _: &ClickEvent, _, cx| {
                computers::cancel_pair();
                pane.refresh(cx);
            }));
            add = add.child(if *asking {
                stacked(
                    tf("pill.kvm.typeCode", &[("name", name)]),
                    t("pill.kvm.typeCodeHintMinute"),
                    div()
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .gap(px(8.))
                        .child(field(&self.code))
                        .child(button("kvm-code-ok".into(), t("pill.kvm.pair"), cx.listener(|pane, _: &ClickEvent, _, cx| pane.submit_code(cx))))
                        .child(cancel),
                )
                .into_any_element()
            } else {
                row(tf("pill.kvm.waiting", &[("name", name)]), t("pill.kvm.waitingHint"), cancel).into_any_element()
            });
        }

        // Por el id: cuando el firewall bloquea la búsqueda.
        add = add.child(stacked(
            t("pill.kvm.byId").into(),
            t("pill.kvm.byIdHint"),
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(8.))
                .child(field(&self.peer_id))
                .when(status.pairing.is_none(), |el| {
                    el.child(button("kvm-pair-id".into(), t("pill.kvm.pair"), cx.listener(|pane, _: &ClickEvent, _, cx| pane.pair_by_id(cx))))
                }),
        ));

        let own: String = status.id.clone();
        let shown = if own.len() > 20 { format!("{}…{}", &own[..10], &own[own.len() - 6..]) } else { own.clone() };
        add.child(row(
            t("pill.kvm.ownId"),
            shown,
            button(
                "kvm-copy-id".into(),
                if self.copied { t("pill.kvm.copied") } else { t("pill.kvm.copy") },
                cx.listener(move |pane, _: &ClickEvent, _, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(own.clone()));
                    pane.copied = true;
                    cx.notify();
                }),
            ),
        ))
    }

    fn notice(&self, notice: &Notice, cx: &mut Context<Self>) -> gpui::AnyElement {
        match notice {
            Notice::Permission => {
                // macOS: llevar a la sección de permisos de la pill.
                let control = if cfg!(target_os = "macos") {
                    button("kvm-permissions".into(), t("pill.kvm.openPermissions"), cx.listener(|_, _: &ClickEvent, _, cx| {
                        computers::dismiss_notice();
                        cx.defer(|cx| crate::settings::open_at(crate::settings::Section::Permissions, cx));
                    }))
                } else {
                    div().into_any_element()
                };
                card()
                    .child(row(t("pill.kvm.permissionTitle"), t("pill.kvm.permissionHint"), control))
                    .into_any_element()
            }
            Notice::Info(text) => note(text.clone(), MUTED).into_any_element(),
            Notice::Error(text) => note(text.clone(), AMBER).into_any_element(),
        }
    }
}

impl Render for ComputersPane {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let status = self.status.clone();
        // Cuando el otro muestra su código, el campo toma el foco (una vez).
        let asking = status.pairing.as_ref().is_some_and(|(_, asking)| *asking);
        if asking && !self.asked {
            self.code.read(cx).focus_handle(cx).focus(window);
        }
        self.asked = asking;

        let enabled = status.enabled;
        let mut body = div().flex().flex_col().child(
            card().child(row(
                t("pill.kvm.enable"),
                if status.starting { t("pill.kvm.starting") } else { t("pill.kvm.enableHint") },
                switch(
                    "kvm-enabled",
                    enabled,
                    cx.listener(move |pane, _: &ClickEvent, _, cx| {
                        computers::set_enabled(!enabled);
                        pane.error = None;
                        pane.refresh(cx);
                    }),
                ),
            )),
        );

        if let Some((name, code)) = &status.pair_code {
            body = body.child(div().pt(px(16.)).child(self.pair_code(name, code)));
        }
        if let Some(notice) = &status.notice {
            body = body.child(div().pt(px(12.)).child(self.notice(notice, cx)));
        }
        if let Some(error) = self.error.clone() {
            body = body.child(note(error.to_string(), AMBER));
        }
        if !status.running {
            return body;
        }

        if !status.peers.is_empty() {
            body = body.child(heading(t("pill.kvm.pairedList")));
            let mut list = div().flex().flex_col().gap(px(12.));
            for (index, peer) in status.peers.iter().enumerate() {
                list = list.child(self.peer(index, peer, cx));
            }
            body = body.child(list);
        }

        let clipboard = status.clipboard;
        body.child(heading(t("pill.kvm.add")))
            .child(self.add(&status, cx))
            .child(heading(t("pill.kvm.more")))
            .child(card().child(row(
                t("pill.kvm.clipboard"),
                t("pill.kvm.clipboardHint"),
                switch(
                    "kvm-clipboard",
                    clipboard,
                    cx.listener(move |pane, _: &ClickEvent, _, cx| {
                        computers::set_clipboard(!clipboard);
                        pane.refresh(cx);
                    }),
                ),
            )))
            .child(note(t(if cfg!(target_os = "macos") { "pill.kvm.escapeMac" } else { "pill.kvm.escape" }).to_string(), MUTED))
    }
}

/// Un texto suelto bajo las tarjetas.
fn note(text: String, color: u32) -> impl IntoElement {
    div().pt(px(8.)).px(px(4.)).text_size(px(12.)).text_color(hsla(color)).child(text)
}

/// Como `row`, pero el control va debajo del texto (un campo con botones no
/// cabe al lado en una ventana angosta).
fn stacked(title: String, hint: &'static str, control: impl IntoElement) -> impl IntoElement {
    let expressive = crate::theme::expressive().is_some();
    div()
        .flex()
        .flex_col()
        .gap(px(10.))
        .px(px(16.))
        .py(px(if expressive { 10. } else { 14. }))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(4.))
                .child(div().text_size(px(13.)).font_weight(FontWeight::MEDIUM).child(title))
                .child(div().text_size(px(12.)).text_color(hsla(MUTED)).child(hint)),
        )
        .child(control)
}

/// «Mac dice que está a la izquierda de este», según de qué lado de este dice estar.
fn claim_key(side: Side) -> &'static str {
    match side {
        Side::Left => "pill.kvm.claimLeft",
        Side::Right => "pill.kvm.claimRight",
        Side::Top => "pill.kvm.claimTop",
        Side::Bottom => "pill.kvm.claimBottom",
    }
}

/// «el borde derecho», etc.: por dónde se pasa al otro.
pub(crate) fn side_edge(side: Side) -> &'static str {
    match side {
        Side::Left => t("pill.kvm.edgeLeft"),
        Side::Right => t("pill.kvm.edgeRight"),
        Side::Top => t("pill.kvm.edgeTop"),
        Side::Bottom => t("pill.kvm.edgeBottom"),
    }
}

pub(crate) fn side_label(side: Side) -> &'static str {
    match side {
        Side::Left => t("pill.kvm.left"),
        Side::Right => t("pill.kvm.right"),
        Side::Top => t("pill.kvm.top"),
        Side::Bottom => t("pill.kvm.bottom"),
    }
}

/// Este equipo al centro y el otro en uno de los cuatro lados; se toca el lado.
fn side_picker(index: usize, peer: &PeerRow, cx: &mut Context<ComputersPane>) -> impl IntoElement {
    let styled = crate::theme::styled();
    // El lado elegido lleva el color de acento; este equipo, el suave.
    let (pick_bg, pick_fg, here_bg, here_fg, line, over) = match &styled {
        Some(t) => (t.accent, t.on_accent, t.accent_soft, t.on_accent_soft, t.border, t.hover),
        None => (hsla(FILL), hsla(INK), hsla(SURFACE_ON), hsla(TEXT), hsla(FAINT).opacity(0.7), hsla(TEXT).opacity(0.08)),
    };
    let radius = styled.as_ref().map_or(10., |t| t.r_ctl.min(14.));
    let slot = |side: Side, cx: &mut Context<ComputersPane>| {
        let id = peer.id.clone();
        let chosen = peer.side == Some(side);
        let el = div()
            .id(SharedString::from(format!("kvm-side-{index}-{side:?}")))
            .w(px(SLOT_W))
            .h(px(SLOT_H))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .px(px(6.))
            .rounded(px(radius))
            .text_size(px(11.));
        if chosen {
            return el
                .bg(pick_bg)
                .text_color(pick_fg)
                .font_weight(FontWeight::SEMIBOLD)
                .child(div().min_w_0().truncate().child(peer.name.clone()))
                .into_any_element();
        }
        el.border_1()
            .border_color(line)
            .text_color(hsla(MUTED))
            .cursor_pointer()
            .on_click(cx.listener(move |pane, _: &ClickEvent, _, cx| {
                computers::set_side(&id, side);
                pane.refresh(cx);
            }))
            .child(side_label(side))
            .hover_bg(SharedString::from(format!("kvm-side-{index}-{side:?}-fx")), over.opacity(0.), over)
            .into_any_element()
    };
    let here = div()
        .w(px(SLOT_W))
        .h(px(SLOT_H))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .gap(px(6.))
        .rounded(px(radius))
        .bg(here_bg)
        .text_color(here_fg)
        .text_size(px(11.))
        .font_weight(FontWeight::MEDIUM)
        .child(svg().path("icons/monitor.svg").size(px(14.)).flex_none().text_color(here_fg))
        .child(t("pill.kvm.thisOne"));
    let (top, left, right, bottom) = (slot(Side::Top, cx), slot(Side::Left, cx), slot(Side::Right, cx), slot(Side::Bottom, cx));
    div()
        .flex()
        .flex_col()
        .items_center()
        .gap(px(SLOT_GAP))
        .child(top)
        .child(div().flex().items_center().gap(px(SLOT_GAP)).child(left).child(here).child(right))
        .child(bottom)
}
