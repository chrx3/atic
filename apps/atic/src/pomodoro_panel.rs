//! El panel del Pomodoro en el notch (`pomodoro.rs`): el temporizador grande
//! con su anillo, los modos y los botones, lo enfocado hoy y los ajustes.
//!
//! El estado vive en la pill (`Pill::pomodoro`): el panel, el vistazo y el
//! reloj del tab muestran lo mismo. Esc cierra el notch.

use std::time::Instant;

use gpui::{canvas, div, prelude::*, px, rgb, svg, AnyElement, Context, FontWeight, Hsla, KeyDownEvent, MouseButton, MouseDownEvent};

use gpui_m3::theme::type_scale::{self, TypeScaleExt};

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
        // En Expressive, los roles del esquema de la pill; el tomate sigue
        // siendo el color del Pomodoro.
        let m3 = crate::pill_look::content_scheme();
        let (text, muted, faint): (Hsla, Hsla, Hsla) = match m3 {
            Some(s) => (s.on_surface, s.on_surface_variant, s.on_surface_variant),
            None => (rgb(0xf0f0ea).into(), rgb(0x9a9a90).into(), rgb(0x6e6e66).into()),
        };
        let tomato: Hsla = rgb(crate::POMODORO_RED).into();
        let ink = crate::pill_look::ink(0x1a1a18);
        let ring_track = m3.map_or(text.opacity(0.1), |s| s.surface_container_highest);
        let layer = |base: Hsla, on: Hsla, h: crate::hover::Hover| crate::hover::m3_layer(base, on, h);
        let pomo = &self.pomodoro;
        let settings = pomo.settings().clone();
        let (done, focused) = pomo.today();
        let running = pomo.running();
        let progress = pomo.progress(now);

        // En M3 Expressive: el progreso circular ondulado (como
        // `WavyCircularProgress` de gpui-m3), en coral, con su pista separada
        // por un hueco, una forma de la biblioteca de fondo y el tiempo en
        // DISPLAY adentro. Se pinta aquí y no con el componente: este panel se
        // dibuja en la vista de la pill, y la onda continua del componente la
        // redibujaría entera a 30 fps (y su resorte no para con un valor que
        // cambia a cada cuadro). La onda avanza un paso por segundo mientras
        // corre, con el reloj; en pausa queda quieta.
        let wave_phase = if running { pomo.remaining(now).as_secs() as f32 * 0.25 } else { 0.0 };
        let ring_m3 = m3.map(|s| {
            div()
                .size(px(RING))
                .relative()
                .flex()
                .items_center()
                .justify_center()
                .child(
                    gpui_m3::Shape::new(gpui_m3::ShapeName::Cookie12)
                        .size(px(RING - 26.0))
                        .color(if running { s.surface_container_high } else { s.surface_container_highest }),
                )
                .child(
                    canvas(
                        |_, _, _| {},
                        move |bounds, _, window, _| {
                            let (x, y) = (f32::from(bounds.origin.x), f32::from(bounds.origin.y));
                            let (cx_, cy_, r) = (x + RING / 2.0, y + RING / 2.0, RING / 2.0 - RING_W);
                            let center = (cx_, cy_, r);
                            // El hueco entre lo que lleva y la pista, en vuelta.
                            let gap = (RING_W + 4.0) / (std::f32::consts::TAU * r);
                            if progress < 1.0 {
                                let from = if progress > 0.0 { progress + gap } else { 0.0 };
                                let to = if progress > 0.0 { 1.0 - gap } else { 1.0 };
                                if let Some(track) = wavy_arc(center, (from, to), RING_W, None) {
                                    window.paint_path(track, ring_track);
                                }
                            }
                            if let Some(done) = wavy_arc(center, (0.0, progress), RING_W, Some(wave_phase)) {
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
                                .type_style(type_scale::DISPLAY_SMALL)
                                .text_color(if running { s.on_surface } else { s.on_surface_variant })
                                .child(format(pomo.remaining(now))),
                        )
                        .child(
                            div()
                                .type_style(type_scale::LABEL_MEDIUM)
                                .text_color(s.on_surface_variant)
                                .child(pomo.mode().label()),
                        ),
                )
        });

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
                            window.paint_path(track, ring_track);
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
        let mode_count = Mode::ALL.len();
        let modes = Mode::ALL.iter().enumerate().map(|(index, &mode)| {
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
                .fx_cx(id, move |el, h, window, cx| {
                    // En Expressive, un grupo de botones conectado de M3: el
                    // elegido se vuelve píldora y el apretado se cuadra.
                    if let Some(s) = m3 {
                        let (round, small) = (MODES_H / 2.0, 4.0);
                        let target = if h.press > 0.5 {
                            [small, small]
                        } else {
                            [
                                if index == 0 || on { round } else { small },
                                if index + 1 == mode_count || on { round } else { small },
                            ]
                        };
                        let [left, right] = gpui_m3::motion::animate(
                            "pomodoro-mode-radii",
                            gpui_m3::Spring::SPATIAL_FAST,
                            target,
                            window,
                            cx,
                        );
                        let (base, fg) = if on {
                            (s.secondary_container, s.on_secondary_container)
                        } else {
                            (s.surface_container_high, s.on_surface_variant)
                        };
                        return el
                            .rounded_tl(px(left.max(0.0)))
                            .rounded_bl(px(left.max(0.0)))
                            .rounded_tr(px(right.max(0.0)))
                            .rounded_br(px(right.max(0.0)))
                            .text_color(fg)
                            .font_weight(FontWeight::MEDIUM)
                            .bg(layer(base, fg, h));
                    }
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
                .fx_cx(id, move |el, h, window, cx| {
                    // En Expressive: play lleno (primary) y los otros tonales.
                    // Play cambia de forma con el estado: corriendo es un
                    // cuadrado redondeado y en pausa, un círculo; apretados, se
                    // cuadran (`SPATIAL_FAST`).
                    if let Some(s) = m3 {
                        let (bg, fg, side) = if big {
                            (layer(s.primary, s.on_primary, h), s.on_primary, 18.)
                        } else {
                            (layer(s.secondary_container, s.on_secondary_container, h), s.on_secondary_container, 15.)
                        };
                        let rest = if big && running { 16.0 } else { size / 2.0 };
                        let radius = crate::hover::m3_press_radius(h, rest, if big { 12.0 } else { 8.0 }, window, cx);
                        return el.rounded(px(radius)).bg(bg).child(svg().path(crate::icons::themed(icon)).size(px(side)).text_color(fg));
                    }
                    if big {
                        el.bg(text.opacity(0.88 + 0.12 * h.t)).child(svg().path(crate::icons::themed(icon)).size(px(18.)).text_color(ink))
                    } else {
                        el.bg(text.opacity(0.06 + 0.08 * h.t)).child(svg().path(crate::icons::themed(icon)).size(px(15.)).text_color(h.mix(muted, text)))
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
                        .child(div().size(px(16.)).rounded_full().bg(match m3 {
                            Some(s) if on => s.on_primary,
                            Some(s) => s.outline,
                            None if on => text,
                            None => muted,
                        }))
                        // En Expressive, el interruptor de M3: encendido en el
                        // acento; apagado, pista alta con contorno.
                        .when_some(m3, |el, s| {
                            if on {
                                el.bg(s.primary)
                            } else {
                                el.bg(s.surface_container_highest).border_1().border_color(s.outline)
                            }
                        })
                        // El interruptor apagado responde al pasar el puntero.
                        .fx(gpui::SharedString::from(format!("{id}-track")), move |el, h| match m3 {
                            Some(_) => el,
                            None if on => el,
                            None => el.bg(text.opacity(0.14 + 0.08 * h.t)),
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
                    .child(
                        div()
                            .text_size(px(12.))
                            .font_weight(FontWeight::SEMIBOLD)
                            // En M3, la barra superior del panel: TITLE_LARGE.
                            .when(m3.is_some(), |el| el.type_style(type_scale::TITLE_LARGE))
                            .child(t("tools.pomodoro.label")),
                    )
                    .child(div().flex_1())
                    .child(div().text_size(px(11.)).text_color(faint).child(tf(
                        "pill.pomodoro.today",
                        &[("n", &done), ("time", &span(focused))],
                    ))),
            )
            .child(div().h(px(GAP)))
            .child(div().flex().justify_center().map(|el| match ring_m3 {
                Some(ring_m3) => el.child(ring_m3),
                None => el.child(ring),
            }))
            .child(div().h(px(GAP)))
            .child(
                div()
                    .h(px(MODES_H))
                    .flex()
                    .justify_center()
                    .gap(px(if m3.is_some() { 2. } else { 6. }))
                    .children(modes.collect::<Vec<_>>()),
            )
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
                    .rounded(px(if m3.is_some() { 16. } else { 14. }))
                    .bg(m3.map_or(text.opacity(0.05), |s| s.surface_container_high))
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

/// Un arco de `from` a `to` (en vueltas, desde arriba y en el sentido del
/// reloj) de grueso `width`. Con `phase`, ondulado como el progreso circular
/// de M3 Expressive: 1,6 px de amplitud cada ~15 px, corrido `phase` ondas.
fn wavy_arc(
    (cx, cy, r): (f32, f32, f32),
    (from, to): (f32, f32),
    width: f32,
    phase: Option<f32>,
) -> Option<gpui::Path<gpui::Pixels>> {
    use std::f32::consts::{FRAC_PI_2, TAU};
    let (from, to) = (from.clamp(0.0, 1.0), to.clamp(0.0, 1.0));
    if to - from <= 0.001 {
        return None;
    }
    const AMPLITUDE: f32 = 1.6;
    const WAVELENGTH: f32 = 15.0;
    let steps = ((to - from) * 160.0).ceil().max(2.0) as usize;
    let mut builder = gpui::PathBuilder::stroke(px(width));
    for i in 0..=steps {
        let turn = from + (to - from) * i as f32 / steps as f32;
        let a = -FRAC_PI_2 + TAU * turn;
        let radius = match phase {
            Some(phase) => r + AMPLITUDE * (TAU * (turn * TAU * r / WAVELENGTH - phase)).sin(),
            None => r,
        };
        let p = gpui::point(px(cx + radius * a.cos()), px(cy + radius * a.sin()));
        if i == 0 {
            builder.move_to(p);
        } else {
            builder.line_to(p);
        }
    }
    builder.build().ok()
}
