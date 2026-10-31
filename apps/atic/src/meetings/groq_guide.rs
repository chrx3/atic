//! La guía para sacar la llave de Groq: un paso a la vez, cada uno con una
//! maqueta dibujada de la consola de Groq y lo que hay que apretar marcado.
//! Las maquetas no copian Groq al pixel: muestran dónde mirar.

use gpui::{div, prelude::*, px, svg, AnyElement, App, ClickEvent, Div, FontWeight, Window};

use super::actions::link_button;
use crate::i18n::{t, tf};
use super::{hsla, FAINT, ITEM, MUTED, RED, TEXT};

/// Las claves del título y de qué hacer, en orden; se traducen al dibujar. La
/// maqueta de cada uno sale de `mock`.
pub(super) const STEPS: [(&str, &str); 6] = [
    ("pill.meetings.groqGuide.step1Title", "pill.meetings.groqGuide.step1Body"),
    ("pill.meetings.groqGuide.step2Title", "pill.meetings.groqGuide.step2Body"),
    ("pill.meetings.groqGuide.step3Title", "pill.meetings.groqGuide.step3Body"),
    ("pill.meetings.groqGuide.step4Title", "pill.meetings.groqGuide.step4Body"),
    ("pill.meetings.groqGuide.step5Title", "pill.meetings.groqGuide.step5Body"),
    ("pill.meetings.groqGuide.step6Title", "pill.meetings.groqGuide.step6Body"),
];

// La consola de Groq es oscura en cualquier tema: así se reconoce.
const BG: u32 = 0x161618;
const PANEL: u32 = 0x232326;
const LINE: u32 = 0x38383e;
const INK: u32 = 0xe8e8ea;
const DIM: u32 = 0x8a8a92;
const ACCENT: u32 = 0xf55036;

const MOCK_W: f32 = 260.;
const MOCK_H: f32 = 150.;

type OnClick = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

/// La tarjeta del paso `step` (0..6) con su maqueta y «Anterior / Siguiente».
pub(super) fn card(step: usize, prev: OnClick, next: OnClick, hide: OnClick) -> AnyElement {
    let step = step.min(STEPS.len() - 1);
    let (title, body) = STEPS[step];
    let (title, body) = (t(title), t(body));
    let last = step + 1 == STEPS.len();
    let dots = (0..STEPS.len()).map(|ix| {
        div()
            .size(px(6.))
            .rounded_full()
            .bg(hsla(if ix == step { TEXT } else { FAINT }).opacity(if ix == step { 1. } else { 0.5 }))
    });
    let nav = div()
        .flex()
        .items_center()
        .gap(px(6.))
        .child(div().flex_1().flex().gap(px(5.)).children(dots))
        .when(step > 0, |el| el.child(link_button("groq-guide-prev", t("pill.meetings.groqGuide.prev"), prev)))
        .child(if last {
            link_button("groq-guide-done", t("pill.meetings.groqGuide.done"), hide)
        } else {
            link_button("groq-guide-next", t("pill.meetings.groqGuide.next"), next)
        });
    let text = div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .gap(px(4.))
        .child(
            div()
                .text_size(px(11.))
                .text_color(hsla(FAINT))
                .child(tf("pill.meetings.groqGuide.progress", &[("step", &(step + 1)), ("total", &STEPS.len())])),
        )
        .child(div().text_size(px(14.)).font_weight(FontWeight::SEMIBOLD).text_color(hsla(TEXT)).child(title))
        .child(div().text_size(px(12.)).line_height(px(17.)).text_color(hsla(MUTED)).child(body))
        .child(div().flex_1())
        .child(nav);
    div()
        .mt(px(8.))
        .p(px(12.))
        .rounded(px(super::card_radius()))
        .bg(hsla(ITEM))
        .flex()
        .gap(px(14.))
        .child(mock(step))
        .child(text)
        .into_any_element()
}

fn mock(step: usize) -> AnyElement {
    match step {
        0 => frame()
            .child(top_bar(false))
            .child(
                centered()
                    .child(small("Create Account or Login", INK))
                    .child(ring(pill("Continue with Google")))
                    .child(pill("Continue with GitHub"))
                    .child(pill("Continue with email")),
            )
            .into_any_element(),
        1 => frame()
            .child(top_bar(true))
            .child(
                div()
                    .flex_1()
                    .p(px(10.))
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(block(28.))
                    .child(div().flex().gap(px(6.)).children((0..3).map(|_| block(34.).flex_1()))),
            )
            .into_any_element(),
        2 => frame()
            .child(top_bar(true))
            .child(
                div()
                    .flex_1()
                    .p(px(10.))
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .child(div().flex_1().child(small("API Keys", INK).font_weight(FontWeight::SEMIBOLD)))
                            .child(ring(pill("+  Create API Key"))),
                    )
                    .child(small("(No keys)", DIM)),
            )
            .into_any_element(),
        3 => frame()
            .child(top_bar(true))
            .child(
                modal()
                    .child(small("Create API Key", INK).font_weight(FontWeight::SEMIBOLD))
                    .child(small("Display Name", DIM))
                    .child(ring(field("Atic|")))
                    .child(small("Expiration", DIM))
                    .child(field("No expiration")),
            )
            .into_any_element(),
        4 => frame()
            .child(top_bar(true))
            .child(
                modal()
                    .child(small(t("pill.meetings.groqGuide.newKey"), INK).font_weight(FontWeight::SEMIBOLD))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .child(field("gsk_••••••••••••••••").flex_1())
                            .child(ring(
                                div()
                                    .size(px(18.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded(px(5.))
                                    .bg(hsla(PANEL))
                                    .child(svg().path("icons/copy.svg").size(px(10.)).text_color(hsla(INK))),
                            )),
                    )
                    .child(small(t("pill.meetings.groqGuide.onlyOnce"), DIM)),
            )
            .into_any_element(),
        // El último paso es en Atic: con los colores de la ventana.
        _ => div()
            .w(px(MOCK_W))
            .h(px(MOCK_H))
            .flex_none()
            .rounded(px(10.))
            .border_1()
            .border_color(hsla(FAINT).opacity(0.4))
            .p(px(12.))
            .flex()
            .flex_col()
            .justify_center()
            .gap(px(8.))
            .child(div().text_size(px(11.)).font_weight(FontWeight::SEMIBOLD).text_color(hsla(TEXT)).child(t("pill.meetings.windowTitle")))
            .child(
                div()
                    .p(px(8.))
                    .rounded(px(8.))
                    .bg(hsla(RED).opacity(0.12))
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(div().text_size(px(9.)).text_color(hsla(RED)).child(t("pill.meetings.groqGuide.needKey")))
                    .child(
                        div()
                            .flex()
                            .gap(px(6.))
                            .child(atic_chip(t("pill.meetings.actions.getKey")))
                            .child(ring(atic_chip(t("pill.meetingsSettings.pasteKey")))),
                    ),
            )
            .into_any_element(),
    }
}

fn frame() -> Div {
    div()
        .w(px(MOCK_W))
        .h(px(MOCK_H))
        .flex_none()
        .rounded(px(10.))
        .overflow_hidden()
        .bg(hsla(BG))
        .border_1()
        .border_color(hsla(LINE))
        .flex()
        .flex_col()
}

/// La barra de Groq: el logo y, a la derecha, las secciones.
fn top_bar(keys_lit: bool) -> Div {
    let keys = small("API Keys", if keys_lit { ACCENT } else { DIM });
    div()
        .h(px(24.))
        .px(px(10.))
        .flex()
        .flex_none()
        .items_center()
        .gap(px(8.))
        .border_b_1()
        .border_color(hsla(LINE))
        .child(div().flex_1().text_size(px(12.)).font_weight(FontWeight::BOLD).text_color(hsla(INK)).child("groq"))
        .child(small("Playground", DIM))
        .child(if keys_lit { ring(keys) } else { keys })
        .child(small("Docs", DIM))
}

fn centered() -> Div {
    div().flex_1().flex().flex_col().items_center().justify_center().gap(px(6.))
}

/// Un diálogo de Groq encima de la página.
fn modal() -> Div {
    div().flex_1().p(px(10.)).child(
        div()
            .p(px(8.))
            .rounded(px(8.))
            .bg(hsla(PANEL))
            .border_1()
            .border_color(hsla(LINE))
            .flex()
            .flex_col()
            .gap(px(4.)),
    )
}

fn small(text: &'static str, color: u32) -> Div {
    div().text_size(px(9.)).text_color(hsla(color)).child(text)
}

fn pill(label: &'static str) -> Div {
    div()
        .h(px(18.))
        .px(px(12.))
        .flex()
        .items_center()
        .rounded(px(9.))
        .border_1()
        .border_color(hsla(LINE))
        .text_size(px(9.))
        .text_color(hsla(INK))
        .child(label)
}

fn field(text: &'static str) -> Div {
    div()
        .h(px(18.))
        .px(px(6.))
        .flex()
        .items_center()
        .rounded(px(5.))
        .border_1()
        .border_color(hsla(LINE))
        .text_size(px(9.))
        .text_color(hsla(INK))
        .child(text)
}

fn block(height: f32) -> Div {
    div().h(px(height)).rounded(px(6.)).bg(hsla(PANEL))
}

fn atic_chip(label: &'static str) -> Div {
    div()
        .h(px(18.))
        .px(px(8.))
        .flex()
        .items_center()
        .rounded(px(9.))
        .bg(hsla(TEXT).opacity(0.08))
        .text_size(px(9.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(hsla(TEXT))
        .child(label)
}

/// Lo que hay que apretar: un anillo del color de Groq alrededor.
fn ring(inner: Div) -> Div {
    div().p(px(2.)).rounded(px(11.)).border_2().border_color(hsla(ACCENT)).child(inner)
}
