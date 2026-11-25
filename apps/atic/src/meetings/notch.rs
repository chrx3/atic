//! Reuniones en el notch: lo que abre su herramienta al hacer clic, como las
//! demás. Arriba lo mismo que el vistazo (Grabar o la grabación en curso, y
//! las últimas reuniones); tocar una muestra su resumen ahí mismo (o su
//! transcripción, si todavía no tiene), con scroll. «Abrir Reuniones ↗» abre
//! la ventana completa.

use std::rc::Rc;

use atic_core::{Recording, RecordingStatus};
use gpui::{
    actions, div, prelude::*, px, svg, AnyElement, App, ClickEvent, Context, ElementId, Entity, EventEmitter,
    FocusHandle, Focusable, Hsla, KeyBinding, MouseButton, SharedString, Window,
};
use gpui_m3::theme::type_scale::{self, TypeScaleExt};
use gpui_m3::{Scheme, ShapeName};

use super::data::Paths;
use crate::i18n::{t, tf};
use super::pill::{body_with, duration, peek_height, row_status, Ink};
use super::pipeline::Job;
use super::recorder::{stopwatch, Stage, Studio};
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
        if crate::pill_look::content_scheme().is_some() {
            let body = match &self.reading {
                Some(_) => READ_HEAD_H + READ_H + M3_GAP,
                None => height_m3(self.studio.read(cx)),
            };
            return BAND_H + body + M3_FOOT_H + 8.0;
        }
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
    let title = rec.map(|r| r.title.clone()).unwrap_or_else(|| t("pill.meetings.notch.untitled").into());
    let Ok(paths) = Paths::resolve() else {
        return Reading { id: id.to_string(), title, kind: t("pill.meetings.notch.noData"), text: t("pill.meetings.notch.noDataText").into() };
    };
    if let Ok(Some(summary)) = atic_core::Summary::load(&paths.summary_path(id)) {
        if !summary.body.trim().is_empty() {
            return Reading { id: id.to_string(), title, kind: t("page.summary.title"), text: summary.body };
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
            return Reading { id: id.to_string(), title, kind: t("page.meetings.transcriptTitle"), text };
        }
    }
    Reading {
        id: id.to_string(),
        title,
        kind: t("pill.meetings.notch.notTranscribed"),
        text: t("pill.meetings.notch.notTranscribedText").into(),
    }
}

impl Render for MeetingsNotch {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(s) = crate::pill_look::content_scheme() {
            return self.render_m3(s, window, cx).into_any_element();
        }
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
                    .child(t("pill.meetings.notch.back"))
                    .into_any_element()
            } else {
                div().text_size(px(12.)).child(t("pill.tray.meetings")).into_any_element()
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
                            .child(t("pill.meetings.notch.open"))
                            .child(gpui::svg().path(crate::icons::themed("icons/arrow-up-right.svg")).size(px(11.)).text_color(fg))
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
            // «Abrir» es un control fijo y el panel abre hacia abajo: va
            // sobre la lista, no al pie, para no saltar cuando cambia de alto.
            .child(foot)
            .child(body)
            .child(div().flex_1())
            .into_any_element()
    }
}

// --- M3 Expressive ------------------------------------------------------------
//
// El panel con los roles del esquema de la pill (`content_scheme`): el título
// en TITLE_LARGE, «Grabar» como botón lleno, las reuniones en una lista
// agrupada (esquinas grandes afuera, chicas entre filas) con su estado como
// chip y la acción como botón tonal, y el texto de una reunión en su
// contenedor. Los clics son los mismos del panel de siempre.

/// El espacio entre los grupos del cuerpo.
const M3_GAP: f32 = 8.0;
/// La fila de la acción principal (un botón de 40 con su aire).
const M3_ACTION_H: f32 = 56.0;
const M3_BUTTON_H: f32 = 40.0;
/// El aviso de la última grabación, como tarjeta, con su espacio.
const M3_NOTICE_H: f32 = 48.0;
/// Una fila de la lista y los 2 px que la separan de la siguiente.
const M3_ROW_H: f32 = 58.0;
/// Grabando: la tarjeta con la forma, el reloj y los medidores.
const M3_CARD_PAD: f32 = 12.0;
const M3_HEAD_H: f32 = 40.0;
const M3_METER_H: f32 = 18.0;
const M3_CAPTION_H: f32 = 22.0;
const M3_CONFIRM_TEXT_H: f32 = 52.0;
const M3_FOOT_H: f32 = 40.0;
const M3_SEGMENTS: usize = 16;

/// El alto del cuerpo en M3, con las mismas partes que `body_m3`.
fn height_m3(studio: &Studio) -> f32 {
    match studio.stage() {
        Stage::Recording => {
            let caption = studio.captions(1).is_some() || studio.live_note().is_some();
            let card = M3_CARD_PAD * 2.0
                + M3_HEAD_H
                + M3_GAP
                + M3_METER_H * 2.0
                + if caption { M3_GAP + M3_CAPTION_H } else { 0.0 };
            card + M3_GAP + M3_ACTION_H
        }
        Stage::Starting | Stage::Stopping => M3_CARD_PAD * 2.0 + M3_HEAD_H + M3_GAP,
        Stage::Confirm => M3_CONFIRM_TEXT_H + M3_ACTION_H,
        Stage::Idle if !studio.can_record() => crate::hover::M3_EMPTY_H,
        Stage::Idle => {
            let notice = if studio.notice().is_some() { M3_NOTICE_H } else { 0.0 };
            let rows = studio.recent().len();
            let list = if rows == 0 { crate::hover::M3_EMPTY_H } else { rows as f32 * M3_ROW_H };
            M3_ACTION_H + notice + list
        }
    }
}

/// El papel de un botón de M3 en el panel.
#[derive(Clone, Copy)]
enum Role {
    /// Lleno, en `primary`: la acción principal.
    Filled,
    /// Lleno, en `error_container`: detener la grabación.
    Danger,
    /// Tonal, en `secondary_container`: la acción de una fila.
    Tonal,
    /// De texto, en `primary`: lo secundario.
    Text,
}

impl Role {
    fn colors(self, s: &Scheme) -> (Hsla, Hsla) {
        match self {
            Role::Filled => (s.primary, s.on_primary),
            Role::Danger => (s.error_container, s.on_error_container),
            Role::Tonal => (s.secondary_container, s.on_secondary_container),
            Role::Text => (s.primary.opacity(0.0), s.primary),
        }
    }
}

/// Lo que va antes del texto de un botón.
#[derive(Clone, Copy)]
enum Glyph {
    None,
    /// El punto rojo de grabar.
    Record,
    /// El cuadrado de detener.
    Stop,
}

/// Un botón de M3 con capa de estado y radio que se cuadra al presionar
/// (`SPATIAL_FAST`). El clic va al bajar el botón, como los del vistazo.
fn button_m3(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    role: Role,
    glyph: Glyph,
    height: f32,
    s: Scheme,
    on_click: impl Fn(&mut App) + 'static,
) -> crate::hover::HoverFx {
    let id = id.into();
    let inner = id.clone();
    let label = label.into();
    crate::hover::hover_fx_cx(id, move |h, window, cx| {
        let radius = crate::hover::m3_press_radius(h, height / 2.0, 8.0, window, cx);
        let (base, fg) = role.colors(&s);
        div()
            .id(inner)
            .h(px(height))
            .px(px(if height >= M3_BUTTON_H { 20. } else { 14. }))
            .when(!matches!(glyph, Glyph::None), |el| el.pl(px(16.)))
            .flex()
            .flex_none()
            .items_center()
            .gap(px(8.))
            .rounded(px(radius))
            .bg(crate::hover::m3_layer(base, fg, h))
            .text_color(fg)
            .type_style(type_scale::LABEL_LARGE)
            .cursor_pointer()
            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                cx.stop_propagation();
                on_click(cx);
            })
            .map(|el| match glyph {
                Glyph::None => el,
                Glyph::Record => el.child(div().size(px(10.)).flex_none().rounded_full().bg(gpui::rgb(super::RED))),
                Glyph::Stop => el.child(div().size(px(10.)).flex_none().rounded(px(2.)).bg(fg)),
            })
            .child(label)
            .into_any_element()
    })
}

/// Cómo se ve el estado de una reunión: su chip y su forma.
#[derive(Clone, Copy, PartialEq)]
enum Tone {
    /// Grabada o transcrita: chip de asistencia, con contorno.
    Neutral,
    /// Transcribiendo o resumiendo.
    Busy,
    /// Con resumen.
    Done,
    Failed,
}

impl Tone {
    /// El fondo y la tinta del chip y de la forma de la fila.
    fn colors(self, s: &Scheme) -> (Hsla, Hsla) {
        match self {
            Tone::Neutral => (s.surface_container_highest, s.on_surface_variant),
            Tone::Busy => (s.tertiary_container, s.on_tertiary_container),
            Tone::Done => (s.primary_container, s.on_primary_container),
            Tone::Failed => (s.error_container, s.on_error_container),
        }
    }
}

/// El chip de estado de una reunión y lo que va a su lado (la duración, o la
/// primera línea del resumen).
fn status_m3(s: &Studio, rec: &Recording, line: Option<String>) -> (String, Tone, Option<String>) {
    match (s.job(&rec.id), rec.status) {
        (Some((Job::Transcribe, p)), _) => {
            (tf("pill.meetings.pill.transcribingPct", &[("pct", &(p * 100.0).round())]), Tone::Busy, None)
        }
        (Some((Job::Summarize, _)), _) | (None, RecordingStatus::Summarizing) => {
            (t("pill.meetings.pill.summarizing").into(), Tone::Busy, None)
        }
        (None, RecordingStatus::Transcribing) => (t("pill.meetings.pill.transcribing").into(), Tone::Busy, None),
        (None, RecordingStatus::Recorded) => {
            (t("page.meetings.status.recorded").into(), Tone::Neutral, Some(duration(rec.duration_secs)))
        }
        (None, RecordingStatus::Transcribed) => (t("page.meetings.status.transcribed").into(), Tone::Neutral, None),
        (None, RecordingStatus::Summarized) => (t("page.meetings.status.summarized").into(), Tone::Done, line),
        (None, RecordingStatus::Error) => (t("pill.meetings.pill.transcribeFailed").into(), Tone::Failed, None),
    }
}

/// Un chip de estado (de asistencia): con contorno si es neutro; si no, en su
/// contenedor.
fn chip_m3(label: impl Into<SharedString>, tone: Tone, s: &Scheme) -> gpui::Div {
    let (bg, fg) = tone.colors(s);
    div()
        .h(px(20.))
        .px(px(8.))
        .flex()
        .flex_none()
        .items_center()
        .rounded(gpui_m3::corner::S)
        .map(|el| match tone {
            Tone::Neutral => el.border_1().border_color(s.outline_variant),
            _ => el.bg(bg),
        })
        .type_style(type_scale::LABEL_SMALL)
        .text_color(if tone == Tone::Neutral { s.on_surface_variant } else { fg })
        .child(label.into())
}

/// Una reunión como fila de una lista agrupada de M3: su forma (galleta si ya
/// tiene resumen), el título, el estado como chip y la acción como botón
/// tonal. Un clic abre su resumen en el notch.
fn row_m3(
    studio: &Entity<Studio>,
    s: &Studio,
    i: usize,
    (rec, line): (Recording, Option<String>),
    (first, last): (bool, bool),
    scheme: Scheme,
    open: super::pill::OpenMeeting,
) -> impl IntoElement {
    let (chip, tone, support) = status_m3(s, &rec, line.clone());
    let (_, action) = row_status(s, &rec, line);
    let (shape_bg, shape_fg) = tone.colors(&scheme);
    let shape = if tone == Tone::Done { ShapeName::Cookie9 } else { ShapeName::Circle };
    let id = rec.id.clone();
    let action = action.map(|(label, job)| {
        let (run, action_id) = (studio.clone(), rec.id.clone());
        button_m3(("meet-n-act", i), label, Role::Tonal, Glyph::None, 32., scheme, move |cx| {
            let id = action_id.clone();
            run.update(cx, |s, cx| match job {
                Job::Transcribe => s.transcribe(id, cx),
                Job::Summarize => s.summarize(id, cx),
            })
        })
    });
    let row = div()
        .id(("meet-n-row", i))
        .h(px(M3_ROW_H - 2.))
        .pl(px(10.))
        .pr(px(10.))
        .flex()
        .items_center()
        .gap(px(12.))
        .cursor_pointer()
        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
            cx.stop_propagation();
            open(id.clone(), cx);
        })
        .child(
            gpui_m3::Shape::new(shape)
                .size(px(36.))
                .color(shape_bg)
                .child(svg().path(crate::icons::themed("icons/audio-lines.svg")).size(px(16.)).text_color(shape_fg)),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(
                    div()
                        .truncate()
                        .type_style(type_scale::BODY_MEDIUM)
                        .text_color(scheme.on_surface)
                        .child(SharedString::from(rec.title.clone())),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .min_w_0()
                        .child(chip_m3(chip, tone, &scheme))
                        .children(support.map(|text| {
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .type_style(type_scale::BODY_SMALL)
                                .text_color(scheme.on_surface_variant)
                                .child(SharedString::from(text))
                        })),
                ),
        )
        .children(action);
    div().pb(px(2.)).child(row.fx_cx(("meet-n-row-fx", i), move |el, h, window, cx| {
        let (top, bottom) = crate::hover::m3_press_radii(h, first, last, crate::hover::M3_GROUP_RADII, window, cx);
        el.rounded_tl(px(top))
            .rounded_tr(px(top))
            .rounded_bl(px(bottom))
            .rounded_br(px(bottom))
            .bg(crate::hover::m3_layer(scheme.surface_container_high, scheme.on_surface, h))
    }))
}

/// Un medidor por segmentos en los colores del esquema; sin pista, «No se graba».
fn meter_m3(label: &'static str, level: f32, on: bool, color: Hsla, s: &Scheme) -> impl IntoElement {
    let lit = if on { (level * M3_SEGMENTS as f32).round() as usize } else { 0 };
    let row = div().h(px(M3_METER_H)).flex().items_center().gap(px(10.)).child(
        div()
            .w(px(62.))
            .flex_none()
            .type_style(type_scale::LABEL_MEDIUM)
            .text_color(s.on_surface_variant)
            .child(label),
    );
    if !on {
        return row.child(
            div().flex_1().type_style(type_scale::BODY_SMALL).text_color(s.on_surface_variant).child(t("pill.meetings.pill.notRecorded")),
        );
    }
    row.child(div().flex_1().flex().items_center().gap(px(2.)).children((0..M3_SEGMENTS).map(|n| {
        div().flex_1().h(px(6.)).rounded(px(3.)).bg(if n < lit { color } else { s.surface_container_highest })
    })))
}

/// La cabeza de la tarjeta de grabación: la forma (su punto late mientras se
/// graba, con el pulso de la grabadora), lo que pasa y el reloj.
fn head_m3(label: &'static str, clock: Option<String>, pulse: f32, s: &Scheme) -> impl IntoElement {
    div()
        .h(px(M3_HEAD_H))
        .flex()
        .items_center()
        .gap(px(12.))
        .child(
            gpui_m3::Shape::new(ShapeName::SoftBurst)
                .size(px(40.))
                .color(s.error_container)
                .child(div().size(px(12.)).rounded_full().bg(s.on_error_container).opacity(pulse)),
        )
        .child(div().flex_1().min_w_0().truncate().type_emphasized(type_scale::TITLE_SMALL).child(label))
        .children(clock.map(|clock| {
            div()
                .flex_none()
                .font_family(crate::fonts::MONO)
                .type_style(type_scale::TITLE_LARGE)
                .text_color(s.on_surface)
                .child(clock)
        }))
}

/// La tarjeta de grabación (`surface_container_high`, esquinas grandes).
fn card_m3(s: &Scheme) -> gpui::Div {
    div()
        .p(px(M3_CARD_PAD))
        .flex()
        .flex_col()
        .gap(px(M3_GAP))
        .rounded(px(20.))
        .bg(s.surface_container_high)
}

fn action_row_m3() -> gpui::Div {
    div().h(px(M3_ACTION_H)).flex().items_center().gap(px(8.))
}

/// El cuerpo del panel en M3, sin una reunión abierta: según la etapa de la
/// grabadora, como `body_with`.
fn body_m3(studio: &Entity<Studio>, scheme: Scheme, open: super::pill::OpenMeeting, cx: &mut App) -> AnyElement {
    let s = studio.read(cx);
    let body = div().px(px(SIDE)).flex().flex_col().w_full();
    match s.stage() {
        Stage::Recording => {
            let ((mic, system), tracks) =
                s.levels().unwrap_or(((0.0, 0.0), super::recorder::Tracks { mic: false, system: false }));
            let me = t("page.meetings.me");
            let caption = s
                .captions(1)
                .and_then(|(lines, partial, _)| partial.or_else(|| lines.last().cloned()))
                .map(|line| (line.me, line.text));
            let note = s.live_note();
            let stop = studio.clone();
            let mut card = card_m3(&scheme)
                .child(head_m3(t("pill.meetings.pill.recording"), s.elapsed().map(stopwatch), s.pulse(), &scheme))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .child(meter_m3(me, mic, tracks.mic, scheme.primary, &scheme))
                        .child(meter_m3(t("pill.meetings.pill.others"), system, tracks.system, scheme.tertiary, &scheme)),
                );
            if caption.is_some() || note.is_some() {
                let (who, text) = match (caption, note) {
                    (Some((true, text)), _) => (Some((me, scheme.primary)), text),
                    (Some((false, text)), _) => (Some((t("page.meetings.others"), scheme.tertiary)), text),
                    (None, Some(note)) => (None, note),
                    (None, None) => (None, String::new()),
                };
                card = card.child(
                    div()
                        .h(px(M3_CAPTION_H))
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .children(who.map(|(who, color)| {
                            div().flex_none().type_style(type_scale::LABEL_MEDIUM).text_color(color).child(who)
                        }))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .type_style(type_scale::BODY_SMALL)
                                .text_color(scheme.on_surface_variant)
                                .child(SharedString::from(text)),
                        ),
                );
            }
            body.child(card)
                .child(div().h(px(M3_GAP)))
                .child(action_row_m3().child(button_m3(
                    "meet-n-stop",
                    t("pill.meetings.pill.stop"),
                    Role::Danger,
                    Glyph::Stop,
                    M3_BUTTON_H,
                    scheme,
                    move |cx| stop.update(cx, |s, cx| s.stop_recording(cx)),
                )))
                .into_any_element()
        }
        Stage::Starting | Stage::Stopping => {
            let label = if s.stage() == Stage::Starting {
                t("pill.meetings.pill.openingAudio")
            } else {
                t("pill.meetings.pill.saving")
            };
            body.child(card_m3(&scheme).child(head_m3(label, s.elapsed().map(stopwatch), 1.0, &scheme)))
                .into_any_element()
        }
        Stage::Confirm => {
            let message = s.confirm_message().unwrap_or_default().to_string();
            let (go, cancel) = (studio.clone(), studio.clone());
            body.child(
                div()
                    .h(px(M3_CONFIRM_TEXT_H))
                    .pt(px(4.))
                    .type_style(type_scale::BODY_SMALL)
                    .text_color(scheme.on_surface_variant)
                    .overflow_hidden()
                    .child(SharedString::from(message)),
            )
            .child(
                action_row_m3()
                    .child(button_m3(
                        "meet-n-go",
                        t("pill.meetings.pill.recordAnyway"),
                        Role::Filled,
                        Glyph::Record,
                        M3_BUTTON_H,
                        scheme,
                        move |cx| go.update(cx, |s, cx| s.start_recording(true, cx)),
                    ))
                    .child(button_m3(
                        "meet-n-cancel",
                        t("pill.settings.cancel"),
                        Role::Text,
                        Glyph::None,
                        M3_BUTTON_H,
                        scheme,
                        move |cx| cancel.update(cx, |s, cx| s.cancel_confirm(cx)),
                    )),
            )
            .into_any_element()
        }
        Stage::Idle if !s.can_record() => {
            crate::hover::m3_empty_state(&scheme, ShapeName::Clover4, "icons/mic-off.svg", t("pill.meetings.notch.noDataText"))
                .h(px(crate::hover::M3_EMPTY_H))
                .into_any_element()
        }
        Stage::Idle => {
            let label = match &s.call() {
                Some(call) => tf("pill.meetings.pill.callRecord", &[("provider", &call.provider)]),
                None => t("pill.meetings.pill.record").to_string(),
            };
            let start = studio.clone();
            let mut body = body.child(action_row_m3().child(button_m3(
                "meet-n-record",
                label,
                Role::Filled,
                Glyph::Record,
                M3_BUTTON_H,
                scheme,
                move |cx| start.update(cx, |s, cx| s.start_recording(false, cx)),
            )));
            if let Some((text, error)) = s.notice() {
                let dismiss = studio.clone();
                let (bg, fg) = if error {
                    (scheme.error_container, scheme.on_error_container)
                } else {
                    (scheme.tertiary_container, scheme.on_tertiary_container)
                };
                body = body.child(
                    div().h(px(M3_NOTICE_H)).pb(px(M3_GAP)).child(
                        div()
                            .id("meet-n-notice")
                            .size_full()
                            .px(px(12.))
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .rounded(px(12.))
                            .bg(bg)
                            .text_color(fg)
                            .cursor_pointer()
                            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                cx.stop_propagation();
                                dismiss.update(cx, |s, cx| s.dismiss_notice(cx));
                            })
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .type_style(type_scale::BODY_SMALL)
                                    .overflow_hidden()
                                    .child(SharedString::from(text.to_string())),
                            )
                            .child(svg().path(crate::icons::themed("icons/x.svg")).size(px(14.)).flex_none().text_color(fg)),
                    ),
                );
            }
            let recent = s.recent().to_vec();
            if recent.is_empty() {
                return body
                    .child(
                        crate::hover::m3_empty_state(&scheme, ShapeName::Cookie9, "icons/audio-lines.svg", t("pill.meetings.pill.empty"))
                            .h(px(crate::hover::M3_EMPTY_H)),
                    )
                    .into_any_element();
            }
            let count = recent.len();
            for (i, entry) in recent.into_iter().enumerate() {
                body = body.child(row_m3(studio, s, i, entry, (i == 0, i + 1 == count), scheme, open.clone()));
            }
            body.into_any_element()
        }
    }
}

impl MeetingsNotch {
    fn render_m3(&mut self, s: Scheme, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
            .when(reading, |el| {
                el.child(crate::hover::round_button_m3(
                    "meet-notch-back",
                    "icons/chevron-left.svg",
                    t("pill.meetings.notch.backTip"),
                    true,
                    false,
                    s,
                    cx.listener(|view, _: &ClickEvent, _, cx| {
                        view.reading = None;
                        cx.notify();
                    }),
                ))
            })
            // La barra superior del panel: el título en TITLE_LARGE.
            .child(div().flex_none().type_style(type_scale::TITLE_LARGE).child(t("pill.tray.meetings")))
            .child(div().flex_1())
            .child(crate::hover::pin_button_m3(
                "meet-notch-pin",
                self.pinned,
                s,
                cx.listener(|view, _: &ClickEvent, _, cx| {
                    view.pinned = !view.pinned;
                    cx.notify();
                }),
            ));

        let body = match &self.reading {
            Some(r) => {
                let kind_tone = if r.kind == t("page.summary.title") { Tone::Done } else { Tone::Neutral };
                div()
                    .px(px(SIDE))
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .h(px(READ_HEAD_H))
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .type_emphasized(type_scale::TITLE_SMALL)
                                    .text_color(s.on_surface)
                                    .child(SharedString::from(r.title.clone())),
                            )
                            .child(chip_m3(r.kind, kind_tone, &s)),
                    )
                    .child(
                        div().h(px(READ_H)).rounded(px(16.)).bg(s.surface_container_high).overflow_hidden().child(
                            div()
                                .id("meet-notch-text")
                                .size_full()
                                .p(px(12.))
                                .overflow_y_scroll()
                                .type_style(type_scale::BODY_MEDIUM)
                                .text_color(s.on_surface)
                                .whitespace_normal()
                                .child(SharedString::from(r.text.clone())),
                        ),
                    )
                    .into_any_element()
            }
            None => {
                let view = cx.entity().downgrade();
                let open: super::pill::OpenMeeting = Rc::new(move |id, cx| {
                    let _ = view.update(cx, |view, cx| view.open_meeting(&id, cx));
                });
                body_m3(&self.studio, s, open, cx)
            }
        };

        // «Abrir Reuniones ↗» como botón de texto.
        let foot = div().h(px(M3_FOOT_H)).px(px(SIDE - 6.)).flex().items_center().child(
            div()
                .id("meet-notch-open")
                .h(px(32.))
                .px(px(12.))
                .flex()
                .items_center()
                .gap(px(6.))
                .type_style(type_scale::LABEL_LARGE)
                .text_color(s.primary)
                .cursor_pointer()
                // Con una reunión abierta, la ventana la trae elegida.
                .on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                    match view.reading.as_ref().map(|r| r.id.clone()) {
                        Some(id) => super::show_meeting(id, cx),
                        None => super::show(cx),
                    }
                    cx.emit(MeetingsNotchEvent::Close);
                }))
                .child(t("pill.meetings.notch.open"))
                .child(svg().path(crate::icons::themed("icons/arrow-up-right.svg")).size(px(14.)).text_color(s.primary))
                .fx_cx("meet-notch-open-fx", move |el, h, window, cx| {
                    let radius = crate::hover::m3_press_radius(h, 16.0, 8.0, window, cx);
                    el.rounded(px(radius)).bg(crate::hover::m3_layer(s.primary.opacity(0.0), s.primary, h))
                }),
        );

        // El cuerpo y el pie entran escalonados y cortos (EFFECTS); el texto de
        // una reunión entra de nuevo al abrirla.
        let enter_body = crate::hover::m3_enter(if reading { "meet-n-enter-read" } else { "meet-n-enter-body" }, 0, window, cx);
        let enter_foot = crate::hover::m3_enter("meet-n-enter-foot", 1, window, cx);

        div()
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus)
            .on_action(cx.listener(|view, _: &Dismiss, _, cx| view.dismiss(cx)))
            .size_full()
            .flex()
            .flex_col()
            .pb(px(8.))
            .font_family(crate::fonts::UI)
            .text_color(s.on_surface)
            .child(band)
            // «Abrir Reuniones» es un control fijo y el panel abre hacia
            // abajo: va sobre la lista, no al pie, para no saltar cuando
            // cambia de alto.
            .child(div().flex_none().opacity(enter_foot).child(foot))
            .child(div().flex_none().opacity(enter_body).child(body))
            .child(div().flex_1())
    }
}
