//! El panel del Pomodoro en el notch (`pomodoro.rs`): el temporizador grande
//! con su anillo, los modos y los botones, lo enfocado hoy y los ajustes.
//!
//! El estado vive en la pill (`Pill::pomodoro`): el panel, el vistazo y el
//! reloj del tab muestran lo mismo. Esc cierra el notch.

use std::time::Instant;

use gpui::{canvas, div, prelude::*, px, rgb, svg, AnyElement, Context, FontWeight, Hsla, KeyDownEvent, MouseButton, MouseDownEvent};

use crate::hover::HoverExt;
use crate::i18n::{t, tf};
use crate::pomodoro::{format, span, Mode};
use crate::Pill;

const MARK_GAP: f32 = 40.0;
const SIDE: f32 = 18.0;
const RING: f32 = 128.0;
const RING_W: f32 = 6.0;
const MODES_H: f32 = 28.0;
const BUTTONS_H: f32 = 44.0;
const ROW_H: f32 = 30.0;
const GAP: f32 = 12.0;
const SETTING_ROWS: f32 = 6.0;

/// Lo que mide el panel: no cambia con el estado.
pub(crate) fn height() -> f32 {
    crate::clipboard::BAND_H + GAP + RING + GAP + MODES_H + GAP + BUTTONS_H + GAP * 1.5 + SETTING_ROWS * ROW_H + GAP
}

impl Pill {
    pub(crate) fn pomodoro_panel(&self, cx: &mut Context<Self>) -> AnyElement {
        let now = Instant::now();
        let text: Hsla = rgb(0xf0f0ea).into();
        let muted: Hsla = rgb(0x9a9a90).into();
        let faint: Hsla = rgb(0x6e6e66).into();
        let tomato: Hsla = rgb(crate::POMODORO_RED).into();
        let ink: Hsla = rgb(0x1a1a18).into();
        let pomo = &self.pomodoro;
        let settings = pomo.settings().clone();
        let (done, focused) = pomo.today();
        let running = pomo.running();
        let progress = pomo.progress(now);

        // El anillo: la pista tenue y lo que lleva, en coral.
        let ring = div()
            .size(px(RING))
            .relative()
            .child(
                canvas(
                    |_, _, _| {},
                    move |bounds, _, window, _| {
                        let (x, y) = (f32::from(bounds.origin.x), f32::from(bounds.origin.y));
                        let (cx_, cy_, r) = (x + RING / 2.0, y + RING / 2.0, RING / 2.0 - RING_W);
                        if let Some(track) = crate::usage::arc(cx_, cy_, r, 1.0, RING_W) {
                            window.paint_path(track, text.opacity(0.1));
                        }
                        if let Some(done) = crate::usage::arc(cx_, cy_, r, progress, RING_W) {
                            window.paint_path(done, tomato);
                        }
                    },
                )
                .absolute()
                .size_full(),
            )
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .font_family(crate::fonts::MONO)
                            .text_size(px(32.))
                            .text_color(if running { text } else { muted })
                            .child(format(pomo.remaining(now))),
                    )
                    .child(div().text_size(px(11.)).text_color(muted).child(pomo.mode().label())),
            );

        let current = pomo.mode();
        let modes = Mode::ALL.iter().map(|&mode| {
            let on = mode == current;
            let id = gpui::SharedString::from(format!("pomodoro-panel-mode-{mode:?}"));
            div()
                .id(id.clone())
                .h(px(MODES_H))
                .px(px(12.))
                .flex()
                .items_center()
                .rounded(px(MODES_H / 2.0))
                .text_size(px(12.))
                .cursor_pointer()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |pill, _: &MouseDownEvent, _, cx| {
                        pill.pomodoro.set_mode(mode);
                        pill.sync_pomodoro(Instant::now(), cx);
                        cx.notify();
                    }),
                )
                .child(mode.label())
                .fx(id, move |el, h| {
                    if on {
                        el.text_color(text).bg(text.opacity(0.14))
                    } else {
                        el.text_color(h.mix(muted, text)).bg(text.opacity(0.07 * h.t))
                    }
                })
        });

        let button = |id: &'static str, icon: &'static str, big: bool, act: fn(&mut Pill, &mut Context<Pill>), cx: &mut Context<Self>| {
            let size = if big { BUTTONS_H } else { 34.0 };
            div()
                .id(id)
                .size(px(size))
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .cursor_pointer()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |pill, _: &MouseDownEvent, _, cx| {
                        act(pill, cx);
                        pill.sync_pomodoro(Instant::now(), cx);
                        cx.notify();
                    }),
                )
                .fx(id, move |el, h| {
                    if big {
                        el.bg(text.opacity(0.88 + 0.12 * h.t)).child(svg().path(icon).size(px(18.)).text_color(ink))
                    } else {
                        el.bg(text.opacity(0.06 + 0.08 * h.t)).child(svg().path(icon).size(px(15.)).text_color(h.mix(muted, text)))
                    }
                })
        };
        let play = if running { "icons/pause.svg" } else { "icons/play.svg" };

        // Un ajuste con − valor +.
        let stepper = |id: &'static str, label: &'static str, value: String, nudge: fn(&mut Pill, i32), cx: &mut Context<Self>| {
            let step = |suffix: &'static str, delta: i32, glyph: &'static str, cx: &mut Context<Self>| {
                let id = gpui::SharedString::from(format!("{id}-{suffix}"));
                div()
                    .id(id.clone())
                    .size(px(24.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .cursor_pointer()
                    .text_size(px(14.))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |pill, _: &MouseDownEvent, _, cx| {
                            nudge(pill, delta);
                            pill.sync_pomodoro(Instant::now(), cx);
                            cx.notify();
                        }),
                    )
                    .child(glyph)
                    .fx(id, move |el, h| el.text_color(h.mix(muted, text)).bg(text.opacity(0.06 + 0.08 * h.t)))
            };
            div()
                .h(px(ROW_H))
                .flex()
                .items_center()
                .gap(px(8.))
                .child(div().flex_1().text_size(px(12.)).text_color(muted).child(label))
                .child(step("menos", -1, "−", cx))
                .child(div().w(px(64.)).flex().justify_center().text_size(px(12.)).text_color(text).child(value))
                .child(step("mas", 1, "+", cx))
        };
        // Un ajuste de sí o no.
        let toggle = |id: &'static str, label: &'static str, on: bool, set: fn(&mut Pill, bool), cx: &mut Context<Self>| {
            div()
                .id(id)
                .h(px(ROW_H))
                .flex()
                .items_center()
                .cursor_pointer()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |pill, _: &MouseDownEvent, _, cx| {
                        set(pill, !on);
                        cx.notify();
                    }),
                )
                .child(div().flex_1().text_size(px(12.)).text_color(muted).child(label))
                .child(
                    div()
                        .w(px(34.))
                        .h(px(20.))
                        .p(px(2.))
                        .flex()
                        .when(on, |el| el.justify_end())
                        .rounded_full()
                        .bg(if on { tomato } else { text.opacity(0.14) })
                        .child(div().size(px(16.)).rounded_full().bg(if on { text } else { muted }))
                        // El interruptor apagado responde al pasar el puntero.
                        .fx(gpui::SharedString::from(format!("{id}-track")), move |el, h| {
                            if on { el } else { el.bg(text.opacity(0.14 + 0.08 * h.t)) }
                        }),
                )
        };
        let minutes = |m: u32| tf("pill.pomodoro.minutes", &[("n", &m)]);

        div()
            .size_full()
            .track_focus(&self.pomodoro_focus)
            .key_context("PomodoroPanel")
            .on_key_down(cx.listener(|pill, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" {
                    pill.close_panel(true, cx);
                }
            }))
            .flex()
            .flex_col()
            .font_family(crate::fonts::UI)
            .text_color(text)
            // La franja de arriba: la marca, el nombre y lo de hoy.
            .child(
                div()
                    .h(px(crate::clipboard::BAND_H))
                    .flex_none()
                    .flex()
                    .items_center()
                    .pr(px(SIDE))
                    .child(div().w(px(MARK_GAP)).flex_none())
                    .child(div().text_size(px(12.)).font_weight(FontWeight::SEMIBOLD).child(t("tools.pomodoro.label")))
                    .child(div().flex_1())
                    .child(div().text_size(px(11.)).text_color(faint).child(tf(
                        "pill.pomodoro.today",
                        &[("n", &done), ("time", &span(focused))],
                    ))),
            )
            .child(div().h(px(GAP)))
            .child(div().flex().justify_center().child(ring))
            .child(div().h(px(GAP)))
            .child(div().h(px(MODES_H)).flex().justify_center().gap(px(6.)).children(modes.collect::<Vec<_>>()))
            .child(div().h(px(GAP)))
            .child(
                div()
                    .h(px(BUTTONS_H))
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap(px(18.))
                    .child(button("pomodoro-panel-reset", "icons/rotate-ccw.svg", false, |pill, _| pill.pomodoro.reset(), cx))
                    .child(button("pomodoro-panel-play", play, true, |pill, cx| pill.toggle_pomodoro(cx), cx))
                    .child(button("pomodoro-panel-skip", "icons/skip-forward.svg", false, |pill, _| pill.pomodoro.skip(Instant::now()), cx)),
            )
            .child(div().h(px(GAP * 1.5)))
            // Los ajustes, que se guardan al cambiarlos.
            .child(
                div()
                    .mx(px(SIDE - 6.0))
                    .px(px(10.))
                    .rounded(px(14.))
                    .bg(text.opacity(0.05))
                    .flex()
                    .flex_col()
                    .child(stepper("pomodoro-focus", t("pill.pomodoro.focus"), minutes(settings.focus_min), |p, d| p.pomodoro.nudge_minutes(Mode::Focus, d), cx))
                    .child(stepper("pomodoro-short", t("pill.pomodoro.short"), minutes(settings.short_min), |p, d| p.pomodoro.nudge_minutes(Mode::Short, d), cx))
                    .child(stepper("pomodoro-long", t("pill.pomodoro.long"), minutes(settings.long_min), |p, d| p.pomodoro.nudge_minutes(Mode::Long, d), cx))
                    .child(stepper("pomodoro-rounds", t("pill.pomodoro.longEvery"), tf("pill.pomodoro.focuses", &[("n", &settings.rounds)]), |p, d| p.pomodoro.nudge_rounds(d), cx))
                    .child(toggle("pomodoro-auto", t("pill.pomodoro.autoStart"), settings.auto_start, |p, on| p.pomodoro.set_auto_start(on), cx))
                    .child(toggle("pomodoro-sound", t("pill.pomodoro.sound"), settings.sound, |p, on| p.pomodoro.set_sound(on), cx)),
            )
            .into_any_element()
    }
}
