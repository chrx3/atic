//! Elegir el micrófono de las reuniones probándolos: junto a «Grabar», una
//! tarjeta escucha todas las entradas a la vez (sin grabar) y muestra el nivel
//! de cada una. Se habla, se ve cuál se mueve y se elige con un clic.
//!
//! Los Bluetooth no se abren: abrirlos pasa los audífonos a manos libres. Un
//! micrófono que abre pero no entrega audio (un adaptador USB sin nada
//! enchufado) queda marcado como «No entrega audio».

use std::time::{Duration, Instant};

use atic_audio::{InputMonitor, MonitoredInput};
use gpui::{div, prelude::*, px, svg, AnyElement, ClickEvent, Context, ElementId, FontWeight, SharedString};

use crate::i18n::{t, tf};
use super::recorder::{ease, inner_radius, meter, CARD_INSET};
use super::{hsla, MeetingsView, BLUE, FAINT, GREEN, ITEM, MUTED, SURFACE_ON, TEXT};
use crate::hover::{self, mix, HoverExt};

/// Cada cuánto se redibujan los medidores mientras la tarjeta está abierta.
const FRAME: Duration = Duration::from_millis(50);
/// Segmentos de cada medidor.
const SEGMENTS: usize = 14;
/// Plazo antes de declarar que un micrófono no entrega audio.
const SILENT_AFTER: Duration = Duration::from_millis(1500);

/// La tarjeta abierta.
pub(super) struct MicPicker {
    /// `None` mientras se abren los micrófonos.
    monitor: Option<InputMonitor>,
    /// El micrófono guardado (`mic_device_id`); vacío es el del sistema.
    chosen: String,
    /// Los niveles a la vista, suavizados (0..1), uno por entrada.
    shown: Vec<f32>,
    opened: Instant,
}

/// Lo que dice una fila junto al medidor.
#[derive(Debug, PartialEq, Eq)]
enum Status {
    Bluetooth,
    Closed,
    Waiting,
    Dead,
    Live,
}

fn status(input: &MonitoredInput, waited: Duration) -> Status {
    if input.info.is_bluetooth {
        Status::Bluetooth
    } else if !input.listening {
        Status::Closed
    } else if input.alive() {
        Status::Live
    } else if waited < SILENT_AFTER {
        Status::Waiting
    } else {
        Status::Dead
    }
}

/// ¿Es el micrófono guardado? Las configuraciones viejas guardaban el nombre.
fn is_chosen(chosen: &str, input: &MonitoredInput) -> bool {
    !chosen.is_empty() && (input.info.id == chosen || input.info.name == chosen)
}

impl MeetingsView {
    pub(super) fn mic_picker_open(&self) -> bool {
        self.mic_picker.is_some()
    }

    pub(super) fn toggle_mic_picker(&mut self, cx: &mut Context<Self>) {
        if self.mic_picker.take().is_some() {
            cx.notify();
            return;
        }
        let Some(paths) = self.source.as_ref().and_then(|s| s.paths()).cloned() else {
            return;
        };
        let chosen = atic_core::Config::load(&paths.config_path()).mic_device_id;
        self.mic_picker = Some(MicPicker { monitor: None, chosen, shown: Vec::new(), opened: Instant::now() });
        cx.notify();
        cx.spawn(async move |view, cx| {
            let monitor = cx.background_spawn(async { atic_audio::monitor_inputs() }).await;
            let opened = view
                .update(cx, |view, cx| {
                    let Some(picker) = view.mic_picker.as_mut() else {
                        return false;
                    };
                    match monitor {
                        Ok(monitor) => {
                            picker.shown = vec![0.0; monitor.inputs.len()];
                            picker.monitor = Some(monitor);
                            picker.opened = Instant::now();
                        }
                        Err(error) => {
                            tracing::warn!(%error, "no se pudieron listar los micrófonos");
                            view.mic_picker = None;
                        }
                    }
                    cx.notify();
                    view.mic_picker.is_some()
                })
                .unwrap_or(false);
            // Los medidores se mueven solo mientras la tarjeta sigue abierta.
            while opened {
                cx.background_executor().timer(FRAME).await;
                let go = view
                    .update(cx, |view, cx| {
                        let Some(picker) = view.mic_picker.as_mut() else {
                            return false;
                        };
                        if let Some(monitor) = &picker.monitor {
                            for (shown, input) in picker.shown.iter_mut().zip(&monitor.inputs) {
                                *shown = ease(*shown, meter(input.level()));
                            }
                        }
                        cx.notify();
                        true
                    })
                    .unwrap_or(false);
                if !go {
                    break;
                }
            }
        })
        .detach();
    }

    /// Guarda el micrófono y cierra la tarjeta (suelta los micrófonos).
    fn choose_mic(&mut self, id: String, cx: &mut Context<Self>) {
        if let Some(paths) = self.source.as_ref().and_then(|s| s.paths()) {
            super::settings::save_config(&paths.config_path(), move |cfg| cfg.mic_device_id = id);
        }
        self.mic_picker = None;
        cx.notify();
    }

    /// El botón de la barra de arriba, al lado de «Grabar».
    pub(super) fn mic_button(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.source.as_ref().is_some_and(|s| s.paths().is_some()) || self.recording(cx) {
            return None;
        }
        Some(super::icon_button(
            "meetings-mic",
            "icons/mic.svg",
            t("pill.meetings.mic.tooltip"),
            self.mic_picker_open(),
            cx.listener(|v, _: &ClickEvent, _, cx| v.toggle_mic_picker(cx)),
        ))
    }

    /// La tarjeta sobre la lista: cada micrófono con su medidor.
    pub(super) fn mic_card(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let picker = self.mic_picker.as_ref()?;
        let mut card = div()
            .flex_none()
            .m(px(CARD_INSET))
            .mb(px(0.))
            .p(px(12.))
            .rounded(px(inner_radius()))
            .bg(hsla(SURFACE_ON))
            .flex()
            .flex_col()
            .gap(px(4.))
            .child(
                div()
                    .px(px(4.))
                    .pb(px(4.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .child(div().text_size(px(13.)).font_weight(FontWeight::MEDIUM).child(t("pill.meetingsSettings.mic")))
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(hsla(MUTED))
                                    .child(t("pill.meetings.mic.hint")),
                            ),
                    )
                    .child(match crate::theme::expressive() {
                        Some(_) => gpui_m3::IconButton::new("mic-close", "icons/x.svg")
                            .tooltip(t("page.common.close"))
                            .on_click(cx.listener(|v, _: &ClickEvent, _, cx| v.toggle_mic_picker(cx)))
                            .into_any_element(),
                        None => hover::round_button(
                            "mic-close",
                            "icons/x.svg",
                            "",
                            false,
                            hsla(TEXT),
                            hsla(FAINT),
                            cx.listener(|v, _: &ClickEvent, _, cx| v.toggle_mic_picker(cx)),
                        )
                        .into_any_element(),
                    }),
            );
        let Some(monitor) = &picker.monitor else {
            return Some(
                card.child(
                    div().px(px(4.)).py(px(6.)).text_size(px(12.)).text_color(hsla(FAINT)).child(t("pill.meetings.mic.opening")),
                )
                .into_any_element(),
            );
        };
        let system_name = monitor.inputs.iter().find(|i| i.info.is_default).map(|i| i.info.name.clone());
        card = card.child(choice_row(
            ("mic-choice", 0),
            match system_name {
                Some(name) => tf("pill.meetings.mic.systemDefaultNamed", &[("name", &name)]),
                None => t("pill.meetings.mic.systemDefault").into(),
            },
            picker.chosen.is_empty(),
            None,
            cx.listener(|v, _: &ClickEvent, _, cx| v.choose_mic(String::new(), cx)),
        ));
        let waited = picker.opened.elapsed();
        for (ix, input) in monitor.inputs.iter().enumerate() {
            let id = input.info.id.clone();
            let level = picker.shown.get(ix).copied().unwrap_or(0.0);
            card = card.child(choice_row(
                ("mic-choice", ix + 1),
                input.info.name.clone(),
                is_chosen(&picker.chosen, input),
                Some((status(input, waited), level)),
                cx.listener(move |v, _: &ClickEvent, _, cx| v.choose_mic(id.clone(), cx)),
            ));
        }
        Some(card.into_any_element())
    }
}

/// Una opción: check, nombre y, debajo, su medidor o por qué no lo tiene.
fn choice_row(
    id: (&'static str, usize),
    name: String,
    on: bool,
    level: Option<(Status, f32)>,
    on_click: impl Fn(&ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static,
) -> AnyElement {
    let expressive = crate::theme::expressive();
    // Expressive: la marca es un radio de M3 (el clic lo recibe la fila).
    let check = match expressive {
        Some(_) => gpui_m3::Radio::new(ElementId::NamedInteger("mic-radio".into(), id.1 as u64), on)
            .size(px(18.))
            .into_any_element(),
        None => div()
            .size(px(14.))
            .flex_none()
            .when(on, |el| el.child(svg().path("icons/check.svg").size(px(14.)).text_color(hsla(GREEN))))
            .into_any_element(),
    };
    let mut text = div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .gap(px(5.))
        .child(
            div()
                .truncate()
                .text_size(px(if expressive.is_some() { 14. } else { 13. }))
                .child(SharedString::from(name)),
        );
    if let Some((status, level)) = level {
        text = text.child(match status {
            Status::Live | Status::Waiting => bar(level).into_any_element(),
            other => div()
                .text_size(px(11.))
                .text_color(hsla(FAINT))
                .child(match other {
                    Status::Bluetooth => t("pill.meetings.mic.bluetooth"),
                    Status::Dead => t("pill.meetings.mic.noAudio"),
                    _ => t("pill.meetings.mic.cantOpen"),
                })
                .into_any_element(),
        });
    }
    let row = div().id(id).flex().items_center().cursor_pointer().on_click(on_click);
    // Expressive: ítem de lista de M3, 44 de alto; el elegido en el contenedor
    // del acento con esquinas de 12 y los demás con su capa de estado.
    if let Some(t) = expressive {
        let card = hsla(SURFACE_ON);
        let (rest, over) = if on {
            (t.accent_soft, mix(t.accent_soft, t.on_accent_soft, 0.08))
        } else {
            (card, mix(card, t.text, 0.08))
        };
        return row
            .min_h(px(44.))
            .px(px(12.))
            .py(px(6.))
            .gap(px(12.))
            .font_family(t.font)
            .when(on, |el| el.text_color(t.on_accent_soft))
            .child(check)
            .child(text)
            .fx(id, move |el, h| {
                let mut pressed = over;
                pressed.a = (over.a + 0.06).min(1.0);
                let r = if on { 12. } else { 12. + 10. * h.t };
                el.bg(mix(mix(rest, over, h.t), pressed, h.press)).rounded(px(r - 4. * h.press))
            })
            .into_any_element();
    }
    row.px(px(8.))
        .py(px(7.))
        .gap(px(8.))
        .rounded(px(10.))
        .child(check)
        .child(text)
        .hover_bg(id, hsla(SURFACE_ON), hsla(ITEM))
        .into_any_element()
}

fn bar(level: f32) -> impl IntoElement {
    let lit = (level * SEGMENTS as f32).round() as usize;
    let mut bar = div().flex().items_center().gap(px(2.));
    for n in 0..SEGMENTS {
        bar = bar.child(
            div()
                .flex_1()
                .h(px(5.))
                .rounded(px(3.))
                .bg(if n < lit { hsla(BLUE) } else { hsla(TEXT).opacity(0.07) }),
        );
    }
    bar
}
