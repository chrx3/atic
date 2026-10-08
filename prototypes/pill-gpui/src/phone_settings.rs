//! La sección Celular de la ventana de Ajustes: vincular con un QR, aceptar
//! al celular que lo escaneó, los vinculados y el portapapeles compartido.
//! El canal vive en `phone.rs`; aquí se mira su estado cada segundo.

use std::sync::Arc;
use std::time::Duration;

use gpui::{div, img, prelude::*, px, App, ClickEvent, Context, Entity, RenderImage, SharedString, Window};

use crate::i18n::{t, tf};
use crate::meetings::settings::{button, card, heading, row, switch};
use crate::phone::{self, Status};
use crate::settings::{hsla, AMBER, MUTED};

pub struct PhonePane {
    status: Status,
    /// El QR del ticket mientras se está vinculando.
    qr: Option<Arc<RenderImage>>,
    busy: bool,
    error: Option<SharedString>,
}

pub fn phone_pane(cx: &mut App) -> Entity<PhonePane> {
    cx.new(|cx| {
        // El estado cambia solo (un celular se conecta o pide vincularse).
        cx.spawn(async move |pane, cx| loop {
            cx.background_executor().timer(Duration::from_secs(1)).await;
            let status = cx.background_spawn(async { phone::status() }).await;
            let alive = pane.update(cx, |pane: &mut PhonePane, cx| {
                if pane.status != status {
                    // Ya se vinculó: el QR sobra.
                    if status.devices.len() > pane.status.devices.len() {
                        pane.qr = None;
                    }
                    pane.status = status;
                    cx.notify();
                }
            });
            if alive.is_err() {
                break;
            }
        })
        .detach();
        PhonePane { status: phone::status(), qr: None, busy: false, error: None }
    })
}

impl PhonePane {
    fn show_qr(&mut self, cx: &mut Context<Self>) {
        self.busy = true;
        self.error = None;
        cx.notify();
        cx.spawn(async move |pane, cx| {
            let result = cx.background_spawn(async { phone::pair_start() }).await;
            let _ = pane.update(cx, |pane, cx| {
                pane.busy = false;
                match result.map(|ticket| phone::qr_image(&ticket)) {
                    Ok(Some(qr)) => pane.qr = Some(qr),
                    Ok(None) => pane.error = Some("QR".into()),
                    Err(error) => pane.error = Some(error.into()),
                }
                pane.status = phone::status();
                cx.notify();
            });
        })
        .detach();
    }

    fn cancel_qr(&mut self, cx: &mut Context<Self>) {
        phone::pair_cancel();
        self.qr = None;
        cx.notify();
    }

    fn answer(&mut self, device_id: String, accept: bool, cx: &mut Context<Self>) {
        if let Err(error) = phone::pair_answer(&device_id, accept) {
            self.error = Some(error.into());
        }
        self.status = phone::status();
        cx.notify();
    }
}

impl Render for PhonePane {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let status = self.status.clone();
        let mut body = div().flex().flex_col();

        if let Some((device_id, name)) = status.pending.clone() {
            let reject_id = device_id.clone();
            body = body.child(
                card().child(row(
                    // El nombre va en la pregunta; la ayuda, abajo.
                    tf("settings.phone.approve", &[("name", &name)]),
                    t("settings.phone.approveHint"),
                    div()
                        .flex()
                        .gap(px(6.))
                        .child(button(
                            "phone-reject".into(),
                            t("settings.phone.reject"),
                            cx.listener(move |pane, _: &ClickEvent, _, cx| pane.answer(reject_id.clone(), false, cx)),
                        ))
                        .child(button(
                            "phone-accept".into(),
                            t("settings.phone.accept"),
                            cx.listener(move |pane, _: &ClickEvent, _, cx| pane.answer(device_id.clone(), true, cx)),
                        )),
                )),
            );
        }

        let pair = match &self.qr {
            Some(qr) => div()
                .flex()
                .flex_col()
                .items_center()
                .gap(px(10.))
                .p(px(16.))
                .child(img(qr.clone()).size(px(220.)).rounded(px(12.)))
                .child(div().text_size(px(12.)).text_color(hsla(MUTED)).child(t("settings.phone.scan")))
                .child(button(
                    "phone-cancel".into(),
                    t("settings.phone.cancel"),
                    cx.listener(|pane, _: &ClickEvent, _, cx| pane.cancel_qr(cx)),
                ))
                .into_any_element(),
            None => row(
                t("settings.phone.pair"),
                t("settings.phone.pairHint"),
                if self.busy {
                    div().text_size(px(12.)).text_color(hsla(MUTED)).child("…").into_any_element()
                } else {
                    button(
                        "phone-pair".into(),
                        t("settings.phone.pairButton"),
                        cx.listener(|pane, _: &ClickEvent, _, cx| pane.show_qr(cx)),
                    )
                    .into_any_element()
                },
            )
            .into_any_element(),
        };
        body = body
            .child(div().px(px(4.)).pb(px(8.)).text_size(px(12.)).text_color(hsla(MUTED)).child(t("pill.phone.hint")))
            .child(card().child(pair));

        if !status.devices.is_empty() {
            let mut devices = card();
            for (index, (id, name, connected)) in status.devices.iter().cloned().enumerate() {
                devices = devices.child(row(
                    name,
                    if connected { t("settings.phone.connected") } else { t("settings.phone.offline") },
                    button(
                        SharedString::from(format!("phone-unpair-{index}")),
                        t("settings.phone.unpair"),
                        cx.listener(move |pane, _: &ClickEvent, _, cx| {
                            if let Err(error) = phone::unpair(&id) {
                                pane.error = Some(error.into());
                            }
                            pane.status = phone::status();
                            cx.notify();
                        }),
                    ),
                ));
            }
            let clipboard = status.clipboard;
            body = body
                .child(heading(t("settings.phone.devices")))
                .child(devices)
                .child(heading(t("settings.phone.share")))
                .child(card().child(row(
                    t("settings.phone.clipboard"),
                    t("settings.phone.clipboardHint"),
                    switch(
                        "phone-clipboard",
                        clipboard,
                        cx.listener(move |pane, _: &ClickEvent, _, cx| {
                            if let Err(error) = phone::set_clipboard(!clipboard) {
                                pane.error = Some(error.into());
                            }
                            pane.status = phone::status();
                            cx.notify();
                        }),
                    ),
                )));
        }

        body.children(
            self.error
                .clone()
                .map(|error| div().pt(px(8.)).px(px(4.)).text_size(px(12.)).text_color(hsla(AMBER)).child(error)),
        )
    }
}
