//! Reuniones en el notch: lo que abre su herramienta al hacer clic, como las
//! demás. Arriba lo mismo que el vistazo (Grabar o la grabación en curso, y
//! las últimas reuniones); tocar una muestra su resumen ahí mismo (o su
//! transcripción, si todavía no tiene), con scroll. «Abrir Reuniones ↗» abre
//! la ventana completa.

use std::rc::Rc;

use atic_core::Recording;
use gpui::{
    actions, div, prelude::*, px, App, ClickEvent, Context, Entity, EventEmitter, FocusHandle, Focusable,
    KeyBinding, SharedString, Window,
};

use super::data::Paths;
use super::pill::{body_with, peek_height, Ink};
use super::recorder::Studio;
use crate::clipboard::{BAND_H, MARK_GAP};
use crate::hover::{pin_button, HoverExt};

actions!(meetings_notch, [Dismiss]);

const KEY_CONTEXT: &str = "MeetingsNotch";
const SIDE: f32 = 14.0;
/// Lo que mide el texto de una reunión abierta (con scroll si es más).
const READ_H: f32 = 300.0;
const READ_HEAD_H: f32 = 44.0;
const FOOT_H: f32 = 34.0;

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([KeyBinding::new("escape", Dismiss, Some(KEY_CONTEXT))]);
}

pub enum MeetingsNotchEvent {
    Close,
}

/// Una reunión abierta en el notch: su título y lo que se lee.
struct Reading {
    id: String,
    title: String,
    /// «Resumen», «Transcripción» o el aviso de que no hay nada.
    kind: &'static str,
    text: String,
}

pub struct MeetingsNotch {
    studio: Entity<Studio>,
    focus: FocusHandle,
    pub pinned: bool,
    reading: Option<Reading>,
}

impl EventEmitter<MeetingsNotchEvent> for MeetingsNotch {}

impl Focusable for MeetingsNotch {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl MeetingsNotch {
    pub fn new(studio: Entity<Studio>, cx: &mut Context<Self>) -> Self {
        // La grabadora cambia (medidores, la lista): el panel se redibuja.
        cx.observe(&studio, |_, _, cx| cx.notify()).detach();
        Self { studio, focus: cx.focus_handle(), pinned: false, reading: None }
    }

    /// Al abrirse desde la herramienta: la lista, no la última leída.
    pub fn reset(&mut self, cx: &mut Context<Self>) {
        self.reading = None;
        cx.notify();
    }

    pub fn desired_height(&self, cx: &App) -> f32 {
        let body = match &self.reading {
            Some(_) => READ_HEAD_H + READ_H,
            None => peek_height(self.studio.read(cx)),
        };
        BAND_H + body + FOOT_H + 8.0
    }

    /// Muestra el resumen de `id` (o su transcripción) en el notch.
    pub fn open_meeting(&mut self, id: &str, cx: &mut Context<Self>) {
        let rec = self.studio.read(cx).recent().iter().map(|(rec, _)| rec).find(|rec| rec.id == id).cloned();
        self.reading = Some(read(id, rec.as_ref()));
        cx.notify();
    }

    fn dismiss(&mut self, cx: &mut Context<Self>) {
        if self.reading.take().is_some() {
            cx.notify();
        } else {
            cx.emit(MeetingsNotchEvent::Close);
        }
    }
}

/// Lo que se lee de una reunión: el resumen, si no la transcripción.
fn read(id: &str, rec: Option<&Recording>) -> Reading {
    let title = rec.map(|r| r.title.clone()).unwrap_or_else(|| "Reunión".into());
    let Ok(paths) = Paths::resolve() else {
        return Reading { id: id.to_string(), title, kind: "Sin datos", text: "No se encontraron los datos de Atic.".into() };
    };
    if let Ok(Some(summary)) = atic_core::Summary::load(&paths.summary_path(id)) {
        if !summary.body.trim().is_empty() {
            return Reading { id: id.to_string(), title, kind: "Resumen", text: summary.body };
        }
    }
    if let Ok(Some(transcript)) = atic_core::Transcript::load(&paths.transcript_path(id)) {
        let text = transcript
            .segments
            .iter()
            .map(|seg| format!("{}: {}", seg.speaker_label(), seg.text.trim()))
            .collect::<Vec<_>>()
            .join("\n\n");
        if !text.is_empty() {
            return Reading { id: id.to_string(), title, kind: "Transcripción", text };
        }
    }
    Reading {
        id: id.to_string(),
        title,
        kind: "Sin transcribir",
        text: "Todavía no tiene transcripción. Vuelve a la lista y toca «Transcribir», o ábrela en Reuniones.".into(),
    }
}

impl Render for MeetingsNotch {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = crate::Palette::dark();
        let ink = Ink { text: palette.text, muted: palette.muted, faint: gpui::rgb(0x8f8f86).into() };
        let reading = self.reading.is_some();

        let band = div()
            .h(px(BAND_H))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(8.))
            .pr(px(SIDE - 4.))
            .child(
                div()
                    .id("meet-notch-mark")
                    .w(px(MARK_GAP))
                    .h_full()
                    .flex_none()
                    .cursor_pointer()
                    .on_click(cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(MeetingsNotchEvent::Close))),
            )
            .child(if reading {
                div()
                    .id("meet-notch-back")
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .text_size(px(12.))
                    .cursor_pointer()
                    .on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                        view.reading = None;
                        cx.notify();
                    }))
                    .child("‹ Reuniones")
                    .into_any_element()
            } else {
                div().text_size(px(12.)).child("Reuniones").into_any_element()
            })
            .child(div().flex_1())
            .child(pin_button(
                "meet-notch-pin",
                self.pinned,
                ink.text,
                ink.faint,
                cx.listener(|view, _: &ClickEvent, _, cx| {
                    view.pinned = !view.pinned;
                    cx.notify();
                }),
            ));

        let body = match &self.reading {
            Some(r) => div()
                .flex()
                .flex_col()
                .child(
                    div()
                        .h(px(READ_HEAD_H))
                        .px(px(SIDE))
                        .flex()
                        .flex_col()
                        .justify_center()
                        .gap(px(2.))
                        .child(
                            div()
                                .truncate()
                                .text_size(px(13.))
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(ink.text)
                                .child(SharedString::from(r.title.clone())),
                        )
                        .child(div().text_size(px(10.5)).text_color(ink.faint).child(r.kind)),
                )
                .child(
                    div()
                        .id("meet-notch-text")
                        .h(px(READ_H))
                        .px(px(SIDE))
                        .overflow_y_scroll()
                        .text_size(px(12.))
                        .line_height(px(18.))
                        .text_color(ink.muted)
                        .whitespace_normal()
                        .child(SharedString::from(r.text.clone())),
                )
                .into_any_element(),
            None => {
                let view = cx.entity().downgrade();
                let open: super::pill::OpenMeeting = Rc::new(move |id, cx| {
                    let _ = view.update(cx, |view, cx| view.open_meeting(&id, cx));
                });
                body_with(&self.studio, ink, open, cx)
            }
        };

        let foot = div()
            .h(px(FOOT_H))
            .px(px(SIDE))
            .flex()
            .items_center()
            .child(
                div()
                    .id("meet-notch-open")
                    .flex()
                    .items_center()
                    .gap(px(5.))
                    .text_size(px(11.))
                    .cursor_pointer()
                    // Con una reunión abierta, la ventana la trae elegida.
                    .on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                        match view.reading.as_ref().map(|r| r.id.clone()) {
                            Some(id) => super::show_meeting(id, cx),
                            None => super::show(cx),
                        }
                        cx.emit(MeetingsNotchEvent::Close);
                    }))
                    .fx("meet-notch-open-fx", move |el, h| {
                        let fg = h.mix(ink.faint, ink.text);
                        el.text_color(fg)
                            .child("Abrir Reuniones")
                            .child(gpui::svg().path("icons/arrow-up-right.svg").size(px(11.)).text_color(fg))
                    }),
            );

        div()
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus)
            .on_action(cx.listener(|view, _: &Dismiss, _, cx| view.dismiss(cx)))
            .size_full()
            .flex()
            .flex_col()
            .pb(px(8.))
            .font_family(crate::fonts::UI)
            .text_color(ink.text)
            .child(band)
            .child(body)
            .child(div().flex_1())
            .child(foot)
    }
}
