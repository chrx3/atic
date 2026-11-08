//! «Vincular y permisos» dentro del panel Celular del notch: lo mismo que la
//! sección Celular de Ajustes (`phone_settings.rs`), a mano y con la piel
//! oscura del notch. Los celulares vinculados (con «Ver pantalla» y
//! «Desvincular»), vincular otro con el QR ahí mismo, aceptar al que lo
//! escaneó, lo que se permite desde el celular y de qué apps llegan avisos.
//!
//! El panel de Agentes/Celular (`agents.rs`) la muestra en vez de los avisos
//! cuando se aprieta su botón de la franja, o sola si no hay ningún celular
//! vinculado. Lee el estado del canal cada segundo mientras está a la vista.

use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::{
    div, img, prelude::*, px, rgb, svg, AnyElement, ClickEvent, Context, Entity, Focusable, Hsla, KeyDownEvent, RenderImage,
    SharedString, Window,
};

use crate::hover::{tip, HoverExt};
use crate::i18n::{t, tf};
use crate::phone::{self, Status};

/// Los márgenes de las filas, como las del panel de Agentes.
const SIDE_PAD: f32 = 8.0;
const HEADER_H: f32 = 30.0;
const ROW_H: f32 = 40.0;
const NOTE_H: f32 = 34.0;
const PENDING_H: f32 = 48.0;
const QR: f32 = 200.0;
const QR_BLOCK_H: f32 = QR + 16.0;
const SWITCH_ROW_H: f32 = 40.0;
const APPS_H: f32 = 34.0;
const ERROR_H: f32 = 24.0;
/// «Desvincular» pide un segundo clic: vale este rato.
const CONFIRM_FOR: Duration = Duration::from_secs(4);
/// Lo que vale el código del QR (`Desktop::pairing_ticket`).
const QR_VALID: Duration = Duration::from_secs(5 * 60);

/// «4:32»: lo que le queda al QR.
fn countdown(left: Duration) -> String {
    let secs = left.as_secs();
    format!("{}:{:02}", secs / 60, secs % 60)
}

/// Los colores de la piel oscura del notch (también los usa `computers_notch.rs`).
#[derive(Clone, Copy)]
pub(crate) struct Ink {
    pub(crate) text: Hsla,
    pub(crate) muted: Hsla,
    pub(crate) faint: Hsla,
    pub(crate) ok: Hsla,
    pub(crate) danger: Hsla,
}

pub(crate) fn ink() -> Ink {
    Ink {
        text: rgb(0xf0f0ea).into(),
        muted: rgb(0x9a9a90).into(),
        faint: rgb(0x6e6e66).into(),
        ok: rgb(0x6cc48a).into(),
        danger: rgb(0xe8826a).into(),
    }
}

pub struct PhoneSetup {
    status: Status,
    /// El QR del ticket mientras se está vinculando.
    qr: Option<Arc<RenderImage>>,
    busy: bool,
    error: Option<SharedString>,
    /// El celular al que se le apretó «Desvincular» una vez, y cuándo.
    confirm: Option<(String, Instant)>,
    /// A la vista en el panel: solo entonces se lee el estado cada segundo.
    visible: bool,
    /// Cuándo se mostró el QR: cuenta lo que le queda y, vencido, ofrece otro.
    qr_at: Option<Instant>,
    /// El celular al que se le está poniendo nombre y su campo.
    renaming: Option<(String, Entity<crate::text_input::TextInput>)>,
}

impl PhoneSetup {
    pub fn new(cx: &mut Context<Self>) -> Self {
        cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(Duration::from_secs(1)).await;
            let Ok(watch) = this.update(cx, |setup: &mut PhoneSetup, _| setup.visible || setup.qr.is_some()) else {
                break;
            };
            if !watch {
                continue;
            }
            let status = cx.background_spawn(async { phone::status() }).await;
            let alive = this.update(cx, |setup: &mut PhoneSetup, cx| {
                setup.take_status(status, cx);
                // La cuenta regresiva del QR avanza aunque nada más cambie.
                if setup.qr.is_some() {
                    cx.notify();
                }
            });
            if alive.is_err() {
                break;
            }
        })
        .detach();
        Self { status: phone::status(), qr: None, busy: false, error: None, confirm: None, visible: false, qr_at: None, renaming: None }
    }

    fn take_status(&mut self, status: Status, cx: &mut Context<Self>) {
        if self.status == status {
            return;
        }
        // Ya se vinculó (uno nuevo, o el mismo reinstalado que reemplaza al de antes): el QR sobra.
        let known = |id: &String| self.status.devices.iter().any(|(old, _, _)| old == id);
        if status.devices.iter().any(|(id, _, _)| !known(id)) {
            crate::release::maybe(self.qr.take());
            self.qr_at = None;
        }
        self.status = status;
        cx.notify();
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        self.status = phone::status();
        cx.notify();
    }

    /// El panel la muestra o la esconde.
    pub fn set_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        if self.visible != visible {
            self.visible = visible;
            if visible {
                self.error = None;
                self.status = phone::status();
            }
            cx.notify();
        }
    }

    /// Se está escribiendo el nombre de un celular: Enter y Esc son de ese campo.
    pub fn renaming(&self) -> bool {
        self.renaming.is_some()
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// Hay algún celular vinculado (sin ninguno, el panel abre aquí).
    pub fn paired(&self) -> bool {
        !self.status.devices.is_empty()
    }

    /// Con el QR a la vista o un celular esperando que lo acepten, el notch
    /// no se cierra al hacer clic fuera: hay que ir al celular a escanear.
    pub fn holds_open(&self) -> bool {
        self.qr.is_some() || self.status.pending.is_some()
    }

    /// Lo que mide todo, sin la franja ni el pie del panel.
    pub fn height(&self) -> f32 {
        let s = &self.status;
        let mut h = HEADER_H;
        h += if s.devices.is_empty() { NOTE_H } else { s.devices.len() as f32 * ROW_H };
        if s.pending.is_some() {
            h += PENDING_H;
        }
        if self.qr.is_some() {
            h += QR_BLOCK_H;
        }
        if !s.devices.is_empty() {
            h += HEADER_H + 5.0 * SWITCH_ROW_H;
            if !s.apps.is_empty() {
                h += HEADER_H + APPS_H;
            }
        }
        if self.error.is_some() {
            h += ERROR_H;
        }
        h + 6.0
    }

    fn show_qr(&mut self, cx: &mut Context<Self>) {
        self.busy = true;
        self.error = None;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx.background_spawn(async { phone::pair_start() }).await;
            let _ = this.update(cx, |setup, cx| {
                setup.busy = false;
                match result.map(|ticket| phone::qr_image(&ticket)) {
                    Ok(Some(qr)) => {
                        crate::release::maybe(setup.qr.replace(qr));
                        setup.qr_at = Some(Instant::now());
                    }
                    Ok(None) => setup.error = Some("QR".into()),
                    Err(error) => setup.error = Some(error.into()),
                }
                setup.refresh(cx);
            });
        })
        .detach();
    }

    /// Deja de mostrar el QR (el ticket deja de valer).
    pub fn cancel_qr(&mut self, cx: &mut Context<Self>) {
        if self.qr.is_some() {
            phone::pair_cancel();
            crate::release::maybe(self.qr.take());
            self.qr_at = None;
            cx.notify();
        }
    }

    fn answer(&mut self, device_id: String, accept: bool, cx: &mut Context<Self>) {
        if let Err(error) = phone::pair_answer(&device_id, accept) {
            self.error = Some(error.into());
        }
        if accept {
            crate::release::maybe(self.qr.take());
        }
        self.refresh(cx);
    }

    /// El primer clic arma la confirmación; el segundo (antes de `CONFIRM_FOR`) desvincula.
    fn unpair(&mut self, device_id: String, cx: &mut Context<Self>) {
        let armed = self.confirm.as_ref().is_some_and(|(id, at)| *id == device_id && at.elapsed() < CONFIRM_FOR);
        if !armed {
            self.confirm = Some((device_id, Instant::now()));
            cx.notify();
            // Si no se confirma, vuelve a «Desvincular».
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(CONFIRM_FOR).await;
                let _ = this.update(cx, |setup, cx| {
                    if setup.confirm.as_ref().is_some_and(|(_, at)| at.elapsed() >= CONFIRM_FOR) {
                        setup.confirm = None;
                        cx.notify();
                    }
                });
            })
            .detach();
            return;
        }
        self.confirm = None;
        if let Err(error) = phone::unpair(&device_id) {
            self.error = Some(error.into());
        }
        self.refresh(cx);
    }

    /// Abre el campo para ponerle nombre, con el de ahora escrito.
    fn start_rename(&mut self, device_id: String, name: String, window: &mut Window, cx: &mut Context<Self>) {
        let c = ink();
        let input = cx.new(|cx| {
            let mut input = crate::text_input::TextInput::new(t("pill.phoneSetup.renamePlaceholder"), c.text, c.faint, c.text, cx);
            input.set_text(name, cx);
            input
        });
        input.read(cx).focus_handle(cx).focus(window);
        self.renaming = Some((device_id, input));
        cx.notify();
    }

    /// Enter guarda el nombre (vacío vuelve al del celular); Esc lo deja como estaba.
    pub fn finish_rename(&mut self, save: bool, cx: &mut Context<Self>) {
        let Some((device_id, input)) = self.renaming.take() else {
            return;
        };
        if save {
            let alias = input.read(cx).text().to_string();
            self.set_result(phone::rename(&device_id, &alias), cx);
        }
        cx.notify();
    }

    fn set_result(&mut self, result: Result<(), String>, cx: &mut Context<Self>) {
        if let Err(error) = result {
            self.error = Some(error.into());
        }
        self.refresh(cx);
    }
}

/// El título de una sección, con algo a la derecha (un botón).
pub(crate) fn header(label: &'static str, right: Option<AnyElement>, c: Ink) -> impl IntoElement {
    div()
        .h(px(HEADER_H))
        .flex_none()
        .flex()
        .items_center()
        .justify_between()
        .pt(px(6.))
        .px(px(SIDE_PAD + 8.))
        // En mayúsculas, como «Recibido del celular» del mismo panel.
        .child(div().text_size(px(10.)).text_color(c.faint).child(label.to_uppercase()))
        .children(right)
}

/// Un botón chico de la piel oscura: ícono opcional y texto. `strong` lo pinta
/// claro (la acción principal); `danger`, en el tono de alerta.
pub(crate) fn chip(
    id: impl Into<SharedString>,
    icon: Option<&'static str>,
    label: impl Into<SharedString>,
    strong: bool,
    danger: bool,
    c: Ink,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut gpui::App) + 'static,
) -> impl IntoElement {
    let id: SharedString = id.into();
    let label: SharedString = label.into();
    let paper: Hsla = rgb(0x1a1a18).into();
    div()
        .id(gpui::ElementId::Name(id.clone()))
        .h(px(24.))
        .px(px(10.))
        .flex()
        .flex_none()
        .items_center()
        .gap(px(5.))
        .rounded(px(12.))
        .text_size(px(11.))
        .cursor_pointer()
        .on_click(on_click)
        .fx(gpui::ElementId::Name(format!("{id}-fx").into()), move |el, h| {
            let (fg, bg) = if strong {
                (paper, c.text.opacity(0.86 + 0.14 * h.t))
            } else if danger {
                (c.danger, c.danger.opacity(0.14 + 0.08 * h.t))
            } else {
                (h.mix(c.muted, c.text), h.mix(c.text.opacity(0.06), c.text.opacity(0.12)))
            };
            el.text_color(fg)
                .bg(bg)
                .children(icon.map(|icon| svg().path(icon).size(px(12.)).text_color(fg)))
                .child(label)
        })
}

/// Un interruptor chico: encendido, claro con la perilla oscura a la derecha.
pub(crate) fn switch(on: bool, c: Ink) -> impl IntoElement {
    let paper: Hsla = rgb(0x1a1a18).into();
    div()
        .w(px(30.))
        .h(px(18.))
        .flex_none()
        .p(px(2.))
        .rounded_full()
        .flex()
        .when(on, |el| el.bg(c.text).justify_end())
        .when(!on, |el| el.bg(c.text.opacity(0.16)))
        .child(div().size(px(14.)).rounded_full().bg(if on { paper } else { c.muted }))
}

impl Render for PhoneSetup {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = ink();
        let status = self.status.clone();
        let mut body = div().flex().flex_col().pb(px(6.));

        // --- Celulares -----------------------------------------------------
        let pair_button = if self.qr.is_some() {
            None
        } else if self.busy {
            Some(div().h(px(24.)).flex().items_center().text_size(px(11.)).text_color(c.muted).child("…").into_any_element())
        } else {
            let label = if status.devices.is_empty() { t("settings.phone.pairButton") } else { t("pill.phoneSetup.pairAnother") };
            Some(
                chip("phone-setup-pair", Some("icons/plus.svg"), label, status.devices.is_empty(), false, c, cx.listener(|setup, _: &ClickEvent, _, cx| setup.show_qr(cx)))
                    .into_any_element(),
            )
        };
        body = body.child(header(t("settings.phone.devices"), pair_button, c));

        if status.devices.is_empty() {
            body = body.child(
                div()
                    .h(px(NOTE_H))
                    .flex_none()
                    .flex()
                    .items_center()
                    .px(px(SIDE_PAD + 8.))
                    .text_size(px(11.))
                    .text_color(c.muted)
                    .child(t("pill.phoneSetup.none")),
            );
        }
        for (index, (id, name, connected)) in status.devices.iter().cloned().enumerate() {
            let id_for_row = id.clone();
            let armed = self.confirm.as_ref().is_some_and(|(armed, at)| *armed == id && at.elapsed() < CONFIRM_FOR);
            let screen_id = id.clone();
            let screen = status.can_screen.contains(&id).then(|| {
                chip(
                    format!("phone-setup-screen-{index}"),
                    Some("icons/monitor.svg"),
                    t("pill.phone.viewScreen"),
                    false,
                    false,
                    c,
                    cx.listener(move |setup, _: &ClickEvent, _, cx| {
                        setup.error = (!phone::request_screen(&screen_id, None)).then(|| t("pill.phone.viewScreenFailed").into());
                        // Si la ventana del celular ya estaba abierta, pasa al frente.
                        crate::phone_screen::raise(cx);
                        cx.notify();
                    }),
                )
            });
            let editing = self.renaming.as_ref().filter(|(renamed, _)| *renamed == id).map(|(_, input)| input.clone());
            let (rename_id, rename_name) = (id.clone(), name.clone());
            let rename = editing.is_none().then(|| {
                crate::hover::round_button(
                    "phone-setup-rename",
                    "icons/pencil.svg",
                    t("pill.phoneSetup.rename"),
                    false,
                    c.text,
                    c.faint,
                    cx.listener(move |setup, _: &ClickEvent, window, cx| setup.start_rename(rename_id.clone(), rename_name.clone(), window, cx)),
                )
            });
            let unpair = chip(
                format!("phone-setup-unpair-{index}"),
                None,
                if armed { t("pill.phoneSetup.confirmUnpair") } else { t("settings.phone.unpair") },
                false,
                armed,
                c,
                cx.listener(move |setup, _: &ClickEvent, _, cx| setup.unpair(id.clone(), cx)),
            );
            body = body.child(
                div()
                    .h(px(ROW_H))
                    .flex_none()
                    .mx(px(SIDE_PAD))
                    .px(px(8.))
                    .flex()
                    .items_center()
                    .gap(px(10.))
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
                            .child(svg().path("icons/smartphone.svg").size(px(13.)).text_color(c.text))
                            // El punto de conectado, en la esquina.
                            .child(
                                div()
                                    .absolute()
                                    .right(px(-1.))
                                    .bottom(px(-1.))
                                    .size(px(8.))
                                    .rounded_full()
                                    .bg(if connected { c.ok } else { c.faint }),
                            ),
                    )
                    .child(match editing {
                        // Renombrando: el campo en su lugar. Enter guarda y Esc lo deja.
                        Some(input) => div()
                            .flex_1()
                            .min_w_0()
                            .h(px(28.))
                            .px(px(10.))
                            .flex()
                            .items_center()
                            .rounded(px(14.))
                            .bg(c.text.opacity(0.08))
                            .text_size(px(12.))
                            .capture_key_down(cx.listener(|setup, event: &KeyDownEvent, _, cx| match event.keystroke.key.as_str() {
                                "enter" => {
                                    cx.stop_propagation();
                                    setup.finish_rename(true, cx);
                                }
                                "escape" => {
                                    cx.stop_propagation();
                                    setup.finish_rename(false, cx);
                                }
                                _ => {}
                            }))
                            .child(input)
                            .into_any_element(),
                        None => div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(1.))
                            .child(div().truncate().text_size(px(12.)).font_weight(gpui::FontWeight::SEMIBOLD).child(name))
                            .child(
                                div()
                                    .text_size(px(10.5))
                                    .text_color(if connected { c.muted } else { c.faint })
                                    .child(if connected { t("settings.phone.connected") } else { t("settings.phone.offline") }),
                            )
                            .into_any_element(),
                    })
                    .children(rename)
                    .when(self.renaming.as_ref().is_some_and(|(renamed, _)| *renamed == id_for_row), |el| {
                        el.child(chip(
                            "phone-setup-rename-save",
                            None,
                            t("pill.phoneSetup.renameSave"),
                            true,
                            false,
                            c,
                            cx.listener(|setup, _: &ClickEvent, _, cx| setup.finish_rename(true, cx)),
                        ))
                    })
                    .children(screen)
                    .child(unpair),
            );
        }

        // Alguien escaneó el QR: se acepta aquí mismo.
        if let Some((device_id, name)) = status.pending.clone() {
            let reject_id = device_id.clone();
            body = body.child(
                div()
                    .h(px(PENDING_H - 6.))
                    .flex_none()
                    .mx(px(SIDE_PAD))
                    .mt(px(3.))
                    .px(px(10.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .rounded(px(12.))
                    .bg(c.text.opacity(0.08))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(1.))
                            .child(div().truncate().text_size(px(12.)).child(tf("settings.phone.approve", &[("name", &name)])))
                            .child(div().truncate().text_size(px(10.5)).text_color(c.muted).child(match status.pending_replaces.clone() {
                                Some(old) => SharedString::from(tf("pill.phoneSetup.replaces", &[("name", &old)])),
                                None => t("settings.phone.approveHint").into(),
                            })),
                    )
                    .child(chip("phone-setup-reject", None, t("settings.phone.reject"), false, false, c, cx.listener(move |setup, _: &ClickEvent, _, cx| setup.answer(reject_id.clone(), false, cx))))
                    .child(chip("phone-setup-accept", None, t("settings.phone.accept"), true, false, c, cx.listener(move |setup, _: &ClickEvent, _, cx| setup.answer(device_id.clone(), true, cx)))),
            );
        }

        // El QR, ahí mismo: a la izquierda; a la derecha, qué hacer con él.
        if let Some(qr) = self.qr.clone() {
            let left = self.qr_at.map_or(QR_VALID, |at| QR_VALID.saturating_sub(at.elapsed()));
            let expired = left.is_zero();
            body = body.child(
                div()
                    .h(px(QR_BLOCK_H))
                    .flex_none()
                    .mx(px(SIDE_PAD))
                    .px(px(8.))
                    .flex()
                    .items_center()
                    .gap(px(14.))
                    .child(
                        div()
                            .size(px(QR))
                            .flex_none()
                            // El margen blanco del QR (el lector lo necesita).
                            .p(px(12.))
                            .rounded(px(16.))
                            .bg(gpui::white())
                            // Vencido ya no sirve: se apaga para que nadie lo escanee.
                            .when(expired, |el| el.opacity(0.18))
                            .child(img(qr).size_full()),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(6.))
                            .child(div().text_size(px(12.)).font_weight(gpui::FontWeight::SEMIBOLD).child(t("settings.phone.pair")))
                            .child(div().text_size(px(11.)).text_color(c.muted).child(t("settings.phone.scan")))
                            .child(if expired {
                                div().text_size(px(11.)).text_color(c.danger).child(t("pill.phoneSetup.qrExpired"))
                            } else {
                                // Las cifras de la letra de la UI ya son de ancho fijo: el contador no baila.
                                div()
                                    .text_size(px(10.5))
                                    .text_color(c.faint)
                                    .child(tf("settings.phone.expires", &[("time", &countdown(left))]))
                            })
                            .child(
                                div().pt(px(4.)).flex().gap(px(6.)).when(expired, |el| {
                                    el.child(chip(
                                        "phone-setup-again",
                                        Some("icons/rotate-cw.svg"),
                                        t("pill.phoneSetup.qrAgain"),
                                        true,
                                        false,
                                        c,
                                        cx.listener(|setup, _: &ClickEvent, _, cx| setup.show_qr(cx)),
                                    ))
                                }).child(chip(
                                    "phone-setup-cancel",
                                    None,
                                    t("settings.phone.cancel"),
                                    false,
                                    false,
                                    c,
                                    cx.listener(|setup, _: &ClickEvent, _, cx| setup.cancel_qr(cx)),
                                )),
                            ),
                    ),
            );
        }

        if !status.devices.is_empty() {
            // --- Lo que se permite desde el celular --------------------------
            let (clipboard, prompt, control, files, bypass) = (status.clipboard, status.prompt, status.control, status.files, status.bypass);
            type Flip = fn(&mut PhoneSetup, (bool, bool, bool, bool, bool), &mut Context<PhoneSetup>);
            let rows: [(&'static str, &'static str, &'static str, bool, Flip); 5] = [
                ("icons/clipboard.svg", t("settings.phone.clipboard"), t("settings.phone.clipboardHint"), clipboard, |s, v, cx| s.set_result(phone::set_clipboard(!v.0), cx)),
                ("icons/square-terminal.svg", t("pill.phone.prompt"), t("pill.phone.promptHint"), prompt, |s, v, cx| s.set_result(phone::set_allowed(!v.1, v.2), cx)),
                ("icons/sliders.svg", t("pill.phone.control"), t("pill.phone.controlHint"), control, |s, v, cx| s.set_result(phone::set_allowed(v.1, !v.2), cx)),
                ("icons/download.svg", t("pill.phone.files"), t("pill.phone.filesHint"), files, |s, v, cx| s.set_result(phone::set_project_access(!v.3, v.4), cx)),
                ("icons/lock.svg", t("pill.phone.bypass"), t("pill.phone.bypassHint"), bypass, |s, v, cx| s.set_result(phone::set_project_access(v.3, !v.4), cx)),
            ];
            let values = (clipboard, prompt, control, files, bypass);
            body = body.child(header(t("pill.phone.allow"), None, c));
            for (i, (icon, label, hint, on, flip)) in rows.into_iter().enumerate() {
                body = body.child(
                    div()
                        .id(("phone-setup-allow", i))
                        .h(px(SWITCH_ROW_H))
                        .flex_none()
                        .mx(px(SIDE_PAD))
                        .px(px(8.))
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .rounded(px(10.))
                        .cursor_pointer()
                        .tooltip(tip(hint))
                        .on_click(cx.listener(move |setup, _: &ClickEvent, _, cx| flip(setup, values, cx)))
                        .child(svg().path(icon).size(px(14.)).flex_none().text_color(if on { c.text } else { c.muted }))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .gap(px(1.))
                                .child(div().truncate().text_size(px(12.)).child(label))
                                .child(div().truncate().text_size(px(10.5)).text_color(c.faint).child(hint)),
                        )
                        .child(switch(on, c))
                        .hover_bg(("phone-setup-allow-fx", i), c.text.opacity(0.0), c.text.opacity(0.06)),
                );
            }

            // --- De qué apps llegan avisos -----------------------------------
            if !status.apps.is_empty() {
                body = body.child(header(t("pill.phone.apps"), Some(div().text_size(px(10.)).text_color(c.faint).child(t("pill.phoneSetup.appsHint")).into_any_element()), c));
                let chips = status.apps.iter().cloned().enumerate().map(|(index, (package, name, shown))| {
                    div()
                        .id(("phone-setup-app", index))
                        .h(px(26.))
                        .px(px(10.))
                        .flex()
                        .flex_none()
                        .items_center()
                        .gap(px(5.))
                        .rounded(px(13.))
                        .text_size(px(11.))
                        .cursor_pointer()
                        .tooltip(tip(if shown { t("pill.phone.appHide") } else { t("pill.phone.appShow") }))
                        .on_click(cx.listener(move |setup, _: &ClickEvent, _, cx| setup.set_result(phone::set_app_muted(&package, shown), cx)))
                        .fx(("phone-setup-app-fx", index), move |el, h| {
                            // Las que se muestran van con fondo y su tilde; las
                            // ocultas, en tenue y con el ojo tachado.
                            if shown {
                                el.text_color(c.text)
                                    .bg(c.text.opacity(0.12 + 0.06 * h.t))
                                    .child(svg().path("icons/check.svg").size(px(11.)).text_color(c.text))
                                    .child(name)
                            } else {
                                let fg = h.mix(c.faint, c.muted);
                                el.text_color(fg)
                                    .bg(c.text.opacity(0.03 + 0.05 * h.t))
                                    .child(svg().path("icons/eye-off.svg").size(px(11.)).text_color(fg))
                                    .child(name)
                            }
                        })
                });
                body = body.child(
                    div()
                        .id("phone-setup-apps")
                        .h(px(APPS_H))
                        .flex_none()
                        .mx(px(SIDE_PAD))
                        .px(px(8.))
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .overflow_x_scroll()
                        .children(chips),
                );
            }
        }

        body = body.children(self.error.clone().map(|error| {
            div()
                .h(px(ERROR_H))
                .flex_none()
                .flex()
                .items_center()
                .px(px(SIDE_PAD + 8.))
                .text_size(px(11.))
                .text_color(c.danger)
                .truncate()
                .child(error)
        }));

        div().id("phone-setup").size_full().overflow_y_scroll().child(body)
    }
}
