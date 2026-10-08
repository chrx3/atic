//! Apariencia de la pill: cuánto se ve el vidrio y cuánto se tapa para leer.
//!
//! El tinte de la pill sobre el vidrio es un compromiso: con poco se ve el
//! desenfoque, con mucho se lee bien el texto sobre fondos claros. Se elige en
//! la sección Apariencia de los Ajustes (`settings.rs`), con la pill a la
//! vista: los cambios se aplican en el cuadro siguiente y se guardan al soltar
//! el deslizador (`<datos de Atic>\pill\appearance.json`).

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Mutex;

use gpui::{
    canvas, div, prelude::*, px, Bounds, ClickEvent, Context, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, Pixels, SharedString, Window,
};
use serde::{Deserialize, Serialize};

use crate::settings::{card, hsla, row, switch_row, text_button, AMBER, FAINT, FILL, MUTED, TEXT, TRACK};

#[derive(Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Appearance {
    /// El escritorio desenfocado detrás de la pill (`glass.rs`).
    pub glass: bool,
    /// Opacidad del tinte con la pill en reposo (0..1).
    pub rest_tint: f32,
    /// Opacidad del tinte con algo para leer: paneles y vistazos.
    pub read_tint: f32,
    /// La letra de la canción y los contadores del tab también se tapan como
    /// un panel. Apagado, se quedan sobre el vidrio con el tinte de reposo.
    pub cover_hang: bool,
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            glass: true,
            rest_tint: crate::GLASS_TINT,
            read_tint: crate::CONTENT_TINT,
            cover_hang: true,
        }
    }
}

const REST_RANGE: (f32, f32) = (0.15, 0.95);
const READ_RANGE: (f32, f32) = (0.5, 1.0);

static CURRENT: Mutex<Option<Appearance>> = Mutex::new(None);

/// La apariencia vigente. La pill la lee en cada cuadro.
pub fn current() -> Appearance {
    match CURRENT.lock() {
        Ok(mut current) => *current.get_or_insert_with(load),
        Err(_) => Appearance::default(),
    }
}

fn apply(look: Appearance, persist: bool) {
    if let Ok(mut current) = CURRENT.lock() {
        *current = Some(look);
    }
    if persist {
        save(&look);
    }
}

fn file() -> Option<PathBuf> {
    crate::paths::file("appearance.json")
}

fn load() -> Appearance {
    let mut look: Appearance = file()
        .and_then(|path| std::fs::read(path).ok())
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default();
    look.rest_tint = look.rest_tint.clamp(REST_RANGE.0, REST_RANGE.1);
    look.read_tint = look.read_tint.clamp(READ_RANGE.0, READ_RANGE.1);
    look
}

fn save(look: &Appearance) {
    let Some(path) = file() else {
        return;
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    match serde_json::to_vec_pretty(look) {
        Ok(bytes) => {
            if let Err(error) = std::fs::write(&path, bytes) {
                eprintln!("apariencia: no se pudo guardar {}: {error}", path.display());
            }
        }
        Err(error) => eprintln!("apariencia: {error}"),
    }
}

// --- Sección de los Ajustes ------------------------------------------------------

const SLIDER_H: f32 = 26.0;
/// La piel de la pill (`Palette::dark`).
const SKIN: u32 = 0x1a1a18;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Knob {
    Rest,
    Read,
}

impl Knob {
    fn range(self) -> (f32, f32) {
        match self {
            Knob::Rest => REST_RANGE,
            Knob::Read => READ_RANGE,
        }
    }

    fn get(self, look: &Appearance) -> f32 {
        match self {
            Knob::Rest => look.rest_tint,
            Knob::Read => look.read_tint,
        }
    }

    fn set(self, look: &mut Appearance, tint: f32) {
        match self {
            Knob::Rest => look.rest_tint = tint,
            Knob::Read => look.read_tint = tint,
        }
    }
}

#[derive(Default)]
pub struct AppearancePane {
    drag: Option<Knob>,
    rest_track: Rc<Cell<Bounds<Pixels>>>,
    read_track: Rc<Cell<Bounds<Pixels>>>,
}

impl AppearancePane {
    fn track(&self, knob: Knob) -> Rc<Cell<Bounds<Pixels>>> {
        match knob {
            Knob::Rest => self.rest_track.clone(),
            Knob::Read => self.read_track.clone(),
        }
    }

    /// Lleva la perilla bajo el cursor. Se guarda al soltar.
    fn drag_to(&mut self, knob: Knob, x: Pixels, cx: &mut Context<Self>) {
        let bounds = self.track(knob).get();
        let width = f32::from(bounds.size.width) - SLIDER_H;
        if width <= 0.0 {
            return;
        }
        // La barra va de 0 a 100 %, como el número de al lado; cada tinte
        // solo se queda dentro de su rango.
        let t = ((f32::from(x - bounds.origin.x) - SLIDER_H / 2.0) / width).clamp(0.0, 1.0);
        let (lo, hi) = knob.range();
        let mut look = current();
        knob.set(&mut look, t.clamp(lo, hi));
        apply(look, false);
        cx.notify();
    }

    fn release(&mut self, cx: &mut Context<Self>) {
        if self.drag.take().is_some() {
            apply(current(), true);
            cx.notify();
        }
    }

    /// Una pill y un panel de muestra sobre un fondo claro (una página) y uno
    /// oscuro, con los tintes elegidos. No desenfoca: eso se ve en la pill.
    fn preview(&self, look: &Appearance) -> impl IntoElement {
        let half = |light: bool| {
            let (page, line) = if light { (0xf4f1ea, 0xc9c4b8) } else { (0x22303d, 0x3d5163) };
            let lines = [0.92, 0.78, 0.98, 0.64, 0.86, 0.72, 0.94].into_iter().map(move |w| {
                div()
                    .h(px(6.))
                    .w(gpui::relative(w))
                    .rounded(px(3.))
                    .bg(hsla(line))
            });
            let hang = if look.cover_hang { look.read_tint } else { look.rest_tint };
            div()
                .flex_1()
                .min_w_0()
                .relative()
                .overflow_hidden()
                .bg(hsla(page))
                .child(
                    div()
                        .absolute()
                        .top(px(14.))
                        .left(px(16.))
                        .right(px(16.))
                        .flex()
                        .flex_col()
                        .gap(px(9.))
                        .children(lines),
                )
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .right_0()
                        .flex()
                        .flex_col()
                        .items_center()
                        // El tab, en reposo.
                        .child(
                            div()
                                .w(px(96.))
                                .h(px(26.))
                                .rounded_b(px(12.))
                                .bg(hsla(SKIN).opacity(look.rest_tint))
                                .flex()
                                .items_center()
                                .justify_center()
                                .gap(px(6.))
                                .text_size(px(10.))
                                .text_color(hsla(TEXT))
                                .child(div().size(px(6.)).rounded_full().bg(hsla(AMBER)))
                                .child("1"),
                        )
                        // Lo que cuelga: la letra.
                        .child(
                            div()
                                .mt(px(6.))
                                .px(px(10.))
                                .py(px(4.))
                                .rounded(px(10.))
                                .bg(hsla(SKIN).opacity(hang))
                                .text_size(px(10.))
                                .text_color(hsla(TEXT))
                                .child("♪ la letra de la canción"),
                        )
                        // Un panel abierto.
                        .child(
                            div()
                                .mt(px(8.))
                                .w(px(150.))
                                .p(px(10.))
                                .rounded(px(12.))
                                .bg(hsla(SKIN).opacity(look.read_tint))
                                .flex()
                                .flex_col()
                                .gap(px(3.))
                                .child(div().text_size(px(11.)).text_color(hsla(TEXT)).child("Un panel abierto"))
                                .child(div().text_size(px(10.)).text_color(hsla(MUTED)).child("Así se lee el texto")),
                        ),
                )
        };
        div()
            .h(px(172.))
            .flex_none()
            .flex()
            .rounded(px(14.))
            .overflow_hidden()
            .border_1()
            .border_color(hsla(TRACK))
            .child(half(true))
            .child(half(false))
    }

    fn slider(&self, knob: Knob, look: &Appearance, cx: &mut Context<Self>) -> impl IntoElement {
        let t = knob.get(look).clamp(0.0, 1.0);
        let store = self.track(knob);
        let width = f32::from(store.get().size.width);
        let fill = if width > SLIDER_H {
            SLIDER_H + (width - SLIDER_H) * t
        } else {
            SLIDER_H
        };
        let dragging = self.drag == Some(knob);
        let id = match knob {
            Knob::Rest => "appearance-rest",
            Knob::Read => "appearance-read",
        };
        div()
            .id(id)
            .relative()
            .h(px(SLIDER_H))
            .rounded(px(SLIDER_H / 2.0))
            .bg(hsla(TRACK))
            .overflow_hidden()
            .cursor_pointer()
            .child(
                canvas(move |bounds, _, _| store.set(bounds), |_, _, _, _| {})
                    .absolute()
                    .size_full(),
            )
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .h_full()
                    .w(px(fill))
                    .rounded(px(SLIDER_H / 2.0))
                    .bg(if dragging { gpui::white() } else { hsla(FILL) }),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |pane, event: &MouseDownEvent, _, cx| {
                    pane.drag = Some(knob);
                    pane.drag_to(knob, event.position.x, cx);
                }),
            )
    }
}

fn percent(tint: f32) -> SharedString {
    format!("{:.0} %", tint * 100.0).into()
}

impl Render for AppearancePane {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let look = current();
        let backdrop = crate::glass::backdrop_available();
        let toggle = |change: fn(&mut Appearance)| {
            move |_: &ClickEvent, window: &mut Window, _: &mut gpui::App| {
                let mut look = current();
                change(&mut look);
                apply(look, true);
                window.refresh();
            }
        };

        div()
            .flex()
            .flex_col()
            .gap(px(16.))
            .on_mouse_move(cx.listener(|pane, event: &MouseMoveEvent, _, cx| {
                if let Some(knob) = pane.drag {
                    if event.pressed_button == Some(MouseButton::Left) {
                        pane.drag_to(knob, event.position.x, cx);
                    } else {
                        pane.release(cx);
                    }
                }
            }))
            .on_mouse_up(MouseButton::Left, cx.listener(|pane, _: &MouseUpEvent, _, cx| pane.release(cx)))
            .on_mouse_up_out(MouseButton::Left, cx.listener(|pane, _: &MouseUpEvent, _, cx| pane.release(cx)))
            .child(self.preview(&look))
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(hsla(FAINT))
                    .child("La muestra no desenfoca: el vidrio se ve en la pill mientras mueves los controles."),
            )
            .child(
                card()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(8.))
                            .child(switch_row(
                                "appearance-glass",
                                "Vidrio esmerilado",
                                "Desenfoca lo que hay detrás de la pill.",
                                look.glass,
                                toggle(|look| look.glass = !look.glass),
                            ))
                            .when(look.glass && !backdrop, |el| {
                                el.child(div().text_size(px(11.)).text_color(hsla(AMBER)).child(
                                    "Windows tiene apagados los efectos de transparencia: la pill usa su color sólido.",
                                ))
                            }),
                    )
                    .child(row(
                        "Tinte en reposo",
                        "Menos tinte deja ver más el vidrio.",
                        Some(percent(look.rest_tint)),
                        self.slider(Knob::Rest, &look, cx),
                    )),
            )
            .child(
                card()
                    .child(row(
                        "Tinte al leer",
                        "Paneles y vistazos. Más alto se lee mejor sobre fondos claros.",
                        Some(percent(look.read_tint)),
                        self.slider(Knob::Read, &look, cx),
                    ))
                    .child(switch_row(
                        "appearance-hang",
                        "Letra y contadores con tinte de lectura",
                        "La letra de la canción y los contadores del tab se tapan como un panel.",
                        look.cover_hang,
                        toggle(|look| look.cover_hang = !look.cover_hang),
                    )),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .child(text_button("appearance-reset", "Restablecer", |_, window, _| {
                        apply(Appearance::default(), true);
                        window.refresh();
                    }))
                    .child(div().flex_1())
                    .child(div().text_size(px(11.)).text_color(hsla(FAINT)).child("Se guarda solo")),
            )
    }
}
