//! «Computadores» dentro del panel Celular del notch: el mouse y el teclado
//! compartidos con otro computador (`computers.rs`), con la piel oscura del
//! notch y las piezas de «Vincular y permisos» (`phone_setup.rs`).
//!
//! Los equipos vinculados con su estado, «Usar este mouse y teclado en él» y
//! un dibujo chico con este equipo al centro y el otro en uno de los cuatro
//! lados (se toca el lado para cambiarlo, como en Ajustes). Cuando otro equipo
//! pide vincularse, el código va arriba, grande. Buscar y vincular siguen en
//! Ajustes → Computadores, a un clic.
//!
//! El panel de Agentes/Celular (`agents.rs`) la muestra con el botón de
//! Computadores de su franja, o sola cuando llega un código. Lee el estado
//! cada segundo mientras está a la vista.

use std::time::Duration;

use atic_kvm::Side;
use gpui::{div, prelude::*, px, rgb, svg, ClickEvent, Context, FontWeight, Hsla, SharedString, Window};

use crate::computers::{self, Notice, PeerRow, Status};
use crate::computers_settings::{side_edge, side_label};
use crate::hover::HoverExt;
use crate::i18n::{t, tf};
use crate::phone_setup::{chip, header, ink, switch, Ink};

/// Los márgenes de las filas, como los de «Vincular y permisos».
const SIDE_PAD: f32 = 8.0;
const HEADER_H: f32 = 30.0;
const NOTE_H: f32 = 34.0;
/// Un equipo: su fila y la del interruptor a la izquierda, el dibujo de los
/// lados a la derecha.
const PEER_H: f32 = 92.0;
const CODE_H: f32 = 112.0;
/// Cada lado del dibujo.
const SLOT_W: f32 = 50.0;
const SLOT_H: f32 = 24.0;
const SLOT_GAP: f32 = 4.0;

pub struct ComputersNotch {
    status: Status,
    /// A la vista en el panel: solo entonces se lee el estado cada segundo.
    visible: bool,
}

impl ComputersNotch {
    pub fn new(cx: &mut Context<Self>) -> Self {
        cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(Duration::from_secs(1)).await;
            let Ok(watch) = this.update(cx, |view: &mut ComputersNotch, _| view.visible) else {
                break;
            };
            if !watch {
                continue;
            }
            let status = cx.background_spawn(async { computers::status() }).await;
            let alive = this.update(cx, |view: &mut ComputersNotch, cx| {
                if view.status != status {
                    view.status = status;
                    cx.notify();
                }
            });
            if alive.is_err() {
                break;
            }
        })
        .detach();
        Self { status: computers::status(), visible: false }
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        self.status = computers::status();
        cx.notify();
    }

    /// El panel la muestra o la esconde.
    pub fn set_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        if self.visible != visible {
            self.visible = visible;
            if visible {
                self.status = computers::status();
            }
            cx.notify();
        }
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// Con un código a la vista el notch no se cierra al hacer clic fuera:
    /// hay que ir al otro equipo a escribirlo.
    pub fn holds_open(&self) -> bool {
        self.status.pair_code.is_some()
    }

    /// Lo que mide todo, sin la franja ni el pie del panel.
    pub fn height(&self) -> f32 {
        let s = &self.status;
        let mut h = HEADER_H;
        if s.pair_code.is_some() {
            h += CODE_H;
        }
        if s.notice.is_some() {
            h += NOTE_H;
        }
        if !s.enabled || !s.running {
            h += NOTE_H;
        } else if s.peers.is_empty() {
            h += NOTE_H;
        } else {
            h += s.peers.len() as f32 * PEER_H;
        }
        if s.running && s.pairing.is_some() {
            h += NOTE_H;
        }
        h + 6.0
    }

    /// El código que hay que escribir en el otro equipo, grande y al centro.
    fn pair_code(&self, name: &str, code: &str, c: Ink) -> impl IntoElement {
        let spaced: String = code.chars().enumerate().flat_map(|(i, ch)| (i == 3).then_some(' ').into_iter().chain([ch])).collect();
        div()
            .h(px(CODE_H - 6.))
            .flex_none()
            .mx(px(SIDE_PAD))
            .mt(px(3.))
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(2.))
            .rounded(px(14.))
            .bg(c.text.opacity(0.08))
            .child(div().text_size(px(12.)).font_weight(FontWeight::SEMIBOLD).child(tf("pill.kvm.codeTitle", &[("name", &name)])))
            .child(
                div()
                    .text_size(px(36.))
                    .line_height(px(46.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .font_family(crate::fonts::MONO)
                    .text_color(c.text)
                    .child(spaced),
            )
            .child(div().text_size(px(11.)).text_color(c.muted).child(t("pill.kvm.codeHint")))
    }

    /// Un texto corto en una fila, con algo a la derecha.
    fn note(text: impl Into<SharedString>, color: Hsla, right: Option<gpui::AnyElement>) -> impl IntoElement {
        div()
            .h(px(NOTE_H))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(8.))
            .px(px(SIDE_PAD + 8.))
            .child(div().flex_1().min_w_0().truncate().text_size(px(11.)).text_color(color).child(text.into()))
            .children(right)
    }

    fn notice(&self, notice: &Notice, c: Ink, cx: &mut Context<Self>) -> gpui::AnyElement {
        match notice {
            Notice::Permission => {
                // macOS: llevar a la sección de permisos de la pill.
                let open = cfg!(target_os = "macos").then(|| {
                    chip(
                        "kvm-notch-permissions",
                        None,
                        t("pill.kvm.openPermissions"),
                        true,
                        false,
                        c,
                        cx.listener(|view, _: &ClickEvent, _, cx| {
                            computers::dismiss_notice();
                            view.refresh(cx);
                            cx.defer(|cx| crate::settings::open_at(crate::settings::Section::Permissions, cx));
                        }),
                    )
                    .into_any_element()
                });
                Self::note(t("pill.kvm.permissionShort"), c.danger, open).into_any_element()
            }
            Notice::Info(text) => Self::note(text.clone(), c.muted, None).into_any_element(),
            Notice::Error(text) => Self::note(text.clone(), c.danger, None).into_any_element(),
        }
    }

    /// Un equipo vinculado: a la izquierda su estado y el interruptor; a la
    /// derecha, de qué lado de este está.
    fn peer(&self, index: usize, peer: &PeerRow, c: Ink, cx: &mut Context<Self>) -> impl IntoElement {
        let status = match (peer.connected, peer.mac) {
            (true, true) => t("pill.kvm.connectedMac"),
            (true, false) => t("pill.kvm.connectedPc"),
            (false, _) => t("pill.kvm.offlineShort"),
        };
        let sharing = peer.sharing;
        let share_id = peer.id.clone();
        let hint: SharedString = if sharing {
            tf("pill.kvm.sharingHint", &[("side", &side_edge(peer.side))]).into()
        } else {
            t("pill.kvm.shareHint").into()
        };
        let who = div()
            .flex()
            .items_center()
            .gap(px(10.))
            .px(px(8.))
            .h(px(40.))
            .child(
                div()
                    .size(px(26.))
                    .flex_none()
                    .relative()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .bg(c.text.opacity(0.08))
                    .child(svg().path(if peer.mac { "icons/laptop.svg" } else { "icons/monitor.svg" }).size(px(13.)).text_color(c.text))
                    // El punto de conectado, en la esquina.
                    .child(
                        div()
                            .absolute()
                            .right(px(-1.))
                            .bottom(px(-1.))
                            .size(px(8.))
                            .rounded_full()
                            .bg(if peer.connected { c.ok } else { c.faint }),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(1.))
                    .child(div().truncate().text_size(px(12.)).font_weight(FontWeight::SEMIBOLD).child(peer.name.clone()))
                    .child(div().truncate().text_size(px(10.5)).text_color(if peer.connected { c.muted } else { c.faint }).child(status)),
            );
        let share = div()
            .id(("kvm-notch-share", index))
            .h(px(36.))
            .px(px(8.))
            .flex()
            .items_center()
            .gap(px(10.))
            .rounded(px(10.))
            .cursor_pointer()
            .tooltip(crate::hover::tip_text(hint))
            .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                computers::set_sharing(&share_id, !sharing);
                view.refresh(cx);
            }))
            .child(svg().path("icons/mouse-pointer-2.svg").size(px(14.)).flex_none().text_color(if sharing { c.text } else { c.muted }))
            .child(div().flex_1().min_w_0().truncate().text_size(px(12.)).child(t("pill.kvm.share")))
            .child(switch(sharing, c))
            .hover_bg(("kvm-notch-share-fx", index), c.text.opacity(0.0), c.text.opacity(0.06));
        div()
            .h(px(PEER_H))
            .flex_none()
            .mx(px(SIDE_PAD))
            .flex()
            .items_center()
            .gap(px(10.))
            .child(div().flex_1().min_w_0().flex().flex_col().gap(px(2.)).child(who).child(share))
            .child(side_picker(index, peer, c, cx))
    }
}

/// Este equipo al centro y el otro en uno de los cuatro lados; se toca el
/// lado. El mismo comportamiento que el de Ajustes (`computers::set_side`).
fn side_picker(index: usize, peer: &PeerRow, c: Ink, cx: &mut Context<ComputersNotch>) -> impl IntoElement {
    let paper: Hsla = rgb(0x1a1a18).into();
    let slot = |side: Side, cx: &mut Context<ComputersNotch>| {
        let id = peer.id.clone();
        let el = div()
            .id(SharedString::from(format!("kvm-notch-side-{index}-{side:?}")))
            .w(px(SLOT_W))
            .h(px(SLOT_H))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .px(px(4.))
            .rounded(px(7.))
            .text_size(px(9.5));
        // El lado elegido, claro con el nombre del otro.
        if peer.side == side {
            return el
                .bg(c.text)
                .text_color(paper)
                .font_weight(FontWeight::SEMIBOLD)
                .child(div().min_w_0().truncate().child(peer.name.clone()))
                .into_any_element();
        }
        el.border_1()
            .border_color(c.text.opacity(0.14))
            .text_color(c.faint)
            .cursor_pointer()
            .tooltip(crate::hover::tip(t("pill.kvm.sideTip")))
            .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                computers::set_side(&id, side);
                view.refresh(cx);
            }))
            .child(side_label(side))
            .hover_bg(SharedString::from(format!("kvm-notch-side-{index}-{side:?}-fx")), c.text.opacity(0.0), c.text.opacity(0.08))
            .into_any_element()
    };
    let here = div()
        .w(px(SLOT_W))
        .h(px(SLOT_H))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .gap(px(4.))
        .rounded(px(7.))
        .bg(c.text.opacity(0.12))
        .text_color(c.text)
        .text_size(px(9.5))
        .child(svg().path("icons/monitor.svg").size(px(10.)).flex_none().text_color(c.text))
        .child(t("pill.kvm.thisOne"));
    let (top, left, right, bottom) = (slot(Side::Top, cx), slot(Side::Left, cx), slot(Side::Right, cx), slot(Side::Bottom, cx));
    div()
        .flex_none()
        .flex()
        .flex_col()
        .items_center()
        .gap(px(SLOT_GAP))
        .child(top)
        .child(div().flex().items_center().gap(px(SLOT_GAP)).child(left).child(here).child(right))
        .child(bottom)
}

impl Render for ComputersNotch {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = ink();
        let status = self.status.clone();
        let settings = chip(
            "kvm-notch-settings",
            Some("icons/settings-2.svg"),
            t("pill.kvm.inSettings"),
            false,
            false,
            c,
            cx.listener(|_, _: &ClickEvent, _, cx| {
                cx.defer(|cx| crate::settings::open_at(crate::settings::Section::Computers, cx));
            }),
        )
        .into_any_element();
        let mut body = div().flex().flex_col().pb(px(6.)).child(header(t("pill.kvm.pairedList"), Some(settings), c));

        if let Some((name, code)) = &status.pair_code {
            body = body.child(self.pair_code(name, code, c));
        }
        if let Some(notice) = &status.notice {
            body = body.child(self.notice(notice, c, cx));
        }
        if !status.enabled {
            body = body.child(Self::note(t("pill.kvm.notchOff"), c.muted, None));
        } else if !status.running {
            let text = if status.starting { t("pill.kvm.starting") } else { "" };
            body = body.child(Self::note(text, c.muted, None));
        } else if status.peers.is_empty() {
            body = body.child(Self::note(t("pill.kvm.notchNone"), c.muted, None));
        } else {
            for (index, peer) in status.peers.iter().enumerate() {
                body = body.child(self.peer(index, peer, c, cx));
            }
        }
        // Vinculando desde Ajustes: el código del otro se escribe allá.
        if let Some((name, asking)) = status.pairing.as_ref().filter(|_| status.running) {
            let text = if *asking { tf("pill.kvm.typeCode", &[("name", name)]) } else { tf("pill.kvm.waiting", &[("name", name)]) };
            body = body.child(Self::note(text, c.muted, None));
        }

        div().id("computers-notch").size_full().overflow_y_scroll().child(body)
    }
}
