//! El vistazo de Reuniones en la pill: lo que se ve al quedarse sobre la
//! herramienta, y cambia con lo que esté pasando.
//!
//! - **Sin grabar**: «Grabar» (o «Llamada en Teams · Grabar» si se detectó
//!   una), el aviso de la última grabación y las últimas reuniones con su
//!   estado. Una reunión transcrita ofrece «Resumir»; una grabada sin
//!   transcribir, «Transcribir».
//! - **Grabando**: el cronómetro, un medidor por pista (así se ve al tiro si
//!   una quedó muda), la última frase de los subtítulos y «Detener».
//!
//! El alto se calcula con las mismas filas que se dibujan (`peek_height`),
//! para que el notch se estire justo lo que mide.

use atic_core::{Recording, RecordingStatus};
use gpui::{div, prelude::*, px, rgb, AnyElement, App, Entity, Hsla, MouseButton, SharedString};

use super::pipeline::Job;
use super::recorder::{stopwatch, Stage, Studio};
use crate::hover::HoverExt;

const SIDE: f32 = 14.0;
const ACTION_H: f32 = 40.0;
const HEAD_H: f32 = 30.0;
const METER_ROW_H: f32 = 18.0;
const CAPTION_H: f32 = 22.0;
const NOTICE_H: f32 = 36.0;
const CONFIRM_TEXT_H: f32 = 36.0;
const ROW_H: f32 = 42.0;
const EMPTY_H: f32 = 30.0;
const SEGMENTS: usize = 16;

/// La ayuda junto al título del vistazo.
pub const MEET_TOOL_HINT: &str = "clic abre la reunión";

const RED: u32 = super::RED;
const BLUE: u32 = super::BLUE;
const LILAC: u32 = super::LILAC;

/// El alto del cuerpo del vistazo, con las mismas filas que `peek_body`.
pub fn peek_height(studio: &Studio) -> f32 {
    match studio.stage() {
        Stage::Recording => {
            let caption = studio.captions(1).is_some() || studio.live_note().is_some();
            HEAD_H + METER_ROW_H * 2.0 + 6.0 + if caption { CAPTION_H } else { 0.0 } + ACTION_H
        }
        Stage::Starting | Stage::Stopping => HEAD_H,
        Stage::Confirm => CONFIRM_TEXT_H + ACTION_H,
        Stage::Idle if !studio.can_record() => EMPTY_H,
        Stage::Idle => {
            let notice = if studio.notice().is_some() { NOTICE_H } else { 0.0 };
            let rows = studio.recent().len();
            let list = if rows == 0 { EMPTY_H } else { rows as f32 * ROW_H };
            ACTION_H + notice + list
        }
    }
}

/// Colores del vistazo, los de la pill.
#[derive(Clone, Copy)]
pub struct Ink {
    pub text: Hsla,
    pub muted: Hsla,
    pub faint: Hsla,
}

/// El cuerpo del vistazo. Los botones mandan a la grabadora; una reunión
/// abre la ventana con ella elegida.
pub fn peek_body(studio: &Entity<Studio>, ink: Ink, cx: &mut App) -> AnyElement {
    let s = studio.read(cx);
    let body = div().flex().flex_col().w_full();
    match s.stage() {
        Stage::Recording => recording(studio, s, ink, body).into_any_element(),
        Stage::Starting | Stage::Stopping => {
            let label = if s.stage() == Stage::Starting { "Abriendo el audio…" } else { "Guardando…" };
            body.child(head_row(label, s.elapsed().map(stopwatch), 1.0, ink)).into_any_element()
        }
        Stage::Confirm => {
            let message = s.confirm_message().unwrap_or_default().to_string();
            let (go, cancel) = (studio.clone(), studio.clone());
            body.child(
                div()
                    .h(px(CONFIRM_TEXT_H))
                    .px(px(SIDE))
                    .text_size(px(11.5))
                    .line_height(px(16.))
                    .text_color(ink.muted)
                    .overflow_hidden()
                    .child(SharedString::from(message)),
            )
            .child(
                actions_row()
                    .child(button("meet-bt-go", "Grabar igual", true, ink, move |cx| {
                        go.update(cx, |s, cx| s.start_recording(true, cx))
                    }))
                    .child(button("meet-bt-cancel", "Cancelar", false, ink, move |cx| {
                        cancel.update(cx, |s, cx| s.cancel_confirm(cx))
                    })),
            )
            .into_any_element()
        }
        Stage::Idle if !s.can_record() => body
            .child(empty_row("No se encontraron los datos de Atic.", ink))
            .into_any_element(),
        Stage::Idle => idle(studio, s, ink, body).into_any_element(),
    }
}

fn recording(studio: &Entity<Studio>, s: &Studio, ink: Ink, body: gpui::Div) -> gpui::Div {
    let ((mic, system), tracks) = s.levels().unwrap_or(((0.0, 0.0), super::recorder::Tracks { mic: false, system: false }));
    let caption = s
        .captions(1)
        .and_then(|(lines, partial, _)| partial.or_else(|| lines.last().cloned()))
        .map(|line| (if line.me { "Yo" } else { "Otros" }, line.text));
    let note = s.live_note();
    let stop = studio.clone();
    body.child(head_row("Grabando", s.elapsed().map(stopwatch), s.pulse(), ink))
        .child(
            div()
                .px(px(SIDE))
                .pb(px(6.))
                .flex()
                .flex_col()
                .child(meter("Yo", mic, tracks.mic, BLUE, ink))
                .child(meter("Los demás", system, tracks.system, LILAC, ink)),
        )
        .when(caption.is_some() || note.is_some(), |el| {
            let (who, text) = match (caption, note) {
                (Some((who, text)), _) => (who, text),
                (None, Some(note)) => ("", note),
                (None, None) => ("", String::new()),
            };
            el.child(
                div()
                    .h(px(CAPTION_H))
                    .px(px(SIDE))
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .text_size(px(11.5))
                    .when(!who.is_empty(), |el| {
                        el.child(
                            div()
                                .flex_none()
                                .text_size(px(10.5))
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(hsla(if who == "Yo" { BLUE } else { LILAC }))
                                .child(who),
                        )
                    })
                    .child(div().flex_1().min_w_0().truncate().text_color(ink.muted).child(SharedString::from(text))),
            )
        })
        .child(actions_row().child(button("meet-stop", "Detener", true, ink, move |cx| {
            stop.update(cx, |s, cx| s.stop_recording(cx))
        })))
}

fn idle(studio: &Entity<Studio>, s: &Studio, ink: Ink, body: gpui::Div) -> gpui::Div {
    let call = s.call();
    let start = studio.clone();
    let label = match &call {
        Some(call) => format!("Llamada en {} · Grabar", call.provider),
        None => "Grabar".to_string(),
    };
    let mut body = body.child(
        actions_row().child(button_owned("meet-record", label, true, ink, move |cx| {
            start.update(cx, |s, cx| s.start_recording(false, cx))
        })),
    );
    if let Some((text, error)) = s.notice() {
        let dismiss = studio.clone();
        body = body.child(
            div()
                .id("meet-notice")
                .h(px(NOTICE_H))
                .px(px(SIDE))
                .flex()
                .items_center()
                .gap(px(8.))
                .cursor_pointer()
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    cx.stop_propagation();
                    dismiss.update(cx, |s, cx| s.dismiss_notice(cx));
                })
                .child(div().size(px(6.)).flex_none().rounded_full().bg(hsla(if error { RED } else { super::AMBER })))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_size(px(11.))
                        .line_height(px(15.))
                        .text_color(ink.muted)
                        .overflow_hidden()
                        .child(SharedString::from(text.to_string())),
                ),
        );
    }
    let recent = s.recent().to_vec();
    if recent.is_empty() {
        return body.child(empty_row("Todavía no hay reuniones.", ink));
    }
    for (i, (rec, line)) in recent.into_iter().enumerate() {
        body = body.child(meeting_row(studio, s, i, rec, line, ink));
    }
    body
}

/// Una reunión reciente: título, estado (o la primera línea del resumen) y
/// lo que se puede hacer con ella.
fn meeting_row(
    studio: &Entity<Studio>,
    s: &Studio,
    i: usize,
    rec: Recording,
    line: Option<String>,
    ink: Ink,
) -> impl IntoElement {
    let job = s.job(&rec.id);
    let (detail, action): (String, Option<(&'static str, Job)>) = match (job, rec.status) {
        (Some((Job::Transcribe, p)), _) => (format!("Transcribiendo… {}%", (p * 100.0).round()), None),
        (Some((Job::Summarize, _)), _) => ("Resumiendo…".into(), None),
        (None, RecordingStatus::Transcribing) => ("Transcribiendo…".into(), None),
        (None, RecordingStatus::Summarizing) => ("Resumiendo…".into(), None),
        (None, RecordingStatus::Recorded) => (duration(rec.duration_secs), Some(("Transcribir", Job::Transcribe))),
        (None, RecordingStatus::Transcribed) => ("Transcrita".into(), Some(("Resumir", Job::Summarize))),
        (None, RecordingStatus::Summarized) => (line.unwrap_or_else(|| "Resumida".into()), None),
        (None, RecordingStatus::Error) => ("No se pudo transcribir".into(), Some(("Reintentar", Job::Transcribe))),
    };
    let id = rec.id.clone();
    let run = studio.clone();
    let action_id = rec.id.clone();
    div()
        .id(("meet-row", i))
        .h(px(ROW_H))
        .mx(px(6.))
        .px(px(SIDE - 6.0))
        .flex()
        .items_center()
        .gap(px(10.))
        .rounded(px(10.))
        .cursor_pointer()
        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
            cx.stop_propagation();
            super::show_meeting(id.clone(), cx);
        })
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .child(
                    div()
                        .truncate()
                        .text_size(px(12.))
                        .text_color(ink.text)
                        .child(SharedString::from(rec.title.clone())),
                )
                .child(
                    div()
                        .truncate()
                        .text_size(px(10.5))
                        .text_color(ink.faint)
                        .child(SharedString::from(detail)),
                ),
        )
        .children(action.map(|(label, job)| {
            button(
                SharedString::from(format!("meet-act-{i}")),
                label,
                false,
                ink,
                move |cx| {
                    let id = action_id.clone();
                    run.update(cx, |s, cx| match job {
                        Job::Transcribe => s.transcribe(id, cx),
                        Job::Summarize => s.summarize(id, cx),
                    })
                },
            )
        }))
        .hover_bg(("meet-row-fx", i), ink.text.opacity(0.0), ink.text.opacity(0.06))
}

pub(crate) fn head_row(label: &'static str, clock: Option<String>, pulse: f32, ink: Ink) -> impl IntoElement {
    div()
        .h(px(HEAD_H))
        .px(px(SIDE))
        .flex()
        .items_center()
        .gap(px(8.))
        .child(div().size(px(8.)).flex_none().rounded_full().bg(hsla(RED).opacity(pulse)))
        .child(div().flex_1().text_size(px(12.)).text_color(ink.text).child(label))
        .children(clock.map(|clock| {
            div()
                .font_family("Cascadia Mono")
                .text_size(px(15.))
                .text_color(ink.text)
                .child(clock)
        }))
}

/// Un medidor plano por segmentos; sin pista, «No se graba».
fn meter(label: &'static str, level: f32, on: bool, color: u32, ink: Ink) -> impl IntoElement {
    let lit = if on { (level * SEGMENTS as f32).round() as usize } else { 0 };
    let row = div()
        .h(px(METER_ROW_H))
        .flex()
        .items_center()
        .gap(px(10.))
        .child(
            div()
                .w(px(62.))
                .flex_none()
                .text_size(px(11.))
                .text_color(if on { ink.muted } else { ink.faint })
                .child(label),
        );
    if !on {
        return row.child(div().flex_1().text_size(px(11.)).text_color(ink.faint).child("No se graba"));
    }
    row.child(div().flex_1().flex().items_center().gap(px(2.)).children((0..SEGMENTS).map(|n| {
        div()
            .flex_1()
            .h(px(5.))
            .rounded(px(2.5))
            .bg(if n < lit { hsla(color) } else { ink.text.opacity(0.08) })
    })))
}

pub(crate) fn actions_row() -> gpui::Div {
    div().h(px(ACTION_H)).px(px(SIDE)).flex().items_center().gap(px(8.))
}

fn empty_row(text: &'static str, ink: Ink) -> impl IntoElement {
    div()
        .h(px(EMPTY_H))
        .px(px(SIDE))
        .flex()
        .items_center()
        .text_size(px(11.5))
        .text_color(ink.faint)
        .child(text)
}

pub(crate) fn button(
    id: impl Into<gpui::ElementId>,
    label: &'static str,
    primary: bool,
    ink: Ink,
    on_click: impl Fn(&mut App) + 'static,
) -> impl IntoElement {
    button_owned(id, label.to_string(), primary, ink, on_click)
}

/// Un botón de píldora; el principal lleva el punto rojo de grabar.
fn button_owned(
    id: impl Into<gpui::ElementId>,
    label: String,
    primary: bool,
    ink: Ink,
    on_click: impl Fn(&mut App) + 'static,
) -> impl IntoElement {
    let id = id.into();
    let red = hsla(RED);
    let (rest, over) = if primary {
        (red.opacity(0.18), red.opacity(0.28))
    } else {
        (ink.text.opacity(0.08), ink.text.opacity(0.14))
    };
    div()
        .id(id.clone())
        .h(px(28.))
        .px(px(13.))
        .flex()
        .flex_none()
        .items_center()
        .gap(px(7.))
        .rounded(px(14.))
        .text_size(px(12.))
        .font_weight(gpui::FontWeight::MEDIUM)
        .text_color(ink.text)
        .cursor_pointer()
        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
            cx.stop_propagation();
            on_click(cx);
        })
        .when(primary, |el| el.child(div().size(px(7.)).rounded_full().bg(red)))
        .child(SharedString::from(label))
        .hover_bg(gpui::ElementId::Name(format!("{id}-fx").into()), rest, over)
}

/// «12 min» o «1 h 05 min».
fn duration(secs: i64) -> String {
    let minutes = (secs.max(0) + 30) / 60;
    if minutes < 60 {
        format!("{} min", minutes.max(1))
    } else {
        format!("{} h {:02} min", minutes / 60, minutes % 60)
    }
}

fn hsla(color: u32) -> Hsla {
    rgb(color).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_duracion_se_redondea_al_minuto() {
        assert_eq!(duration(0), "1 min");
        assert_eq!(duration(12 * 60 + 20), "12 min");
        assert_eq!(duration(65 * 60), "1 h 05 min");
    }
}
