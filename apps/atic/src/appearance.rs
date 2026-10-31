//! Apariencia de la pill: cuánto se ve el vidrio y cuánto se tapa para leer.
//!
//! El tinte de la pill sobre el vidrio es un compromiso: con poco se ve el
//! desenfoque, con mucho se lee bien el texto sobre fondos claros. Se elige en
//! la sección Apariencia de los Ajustes (`settings.rs`), con la pill a la
//! vista: los cambios se aplican en el cuadro siguiente y se guardan al soltar
//! el deslizador (`<datos de Atic>\pill\appearance.json`).
//!
//! Ahí mismo se elige el estilo de las ventanas (Ajustes, Reuniones y el
//! espacio): el de Atic de siempre, Material 3 Expressive o Liquid Glass, como
//! los estilos de Atic Code (`theme::styled`).

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Mutex;

use gpui::{
    canvas, div, prelude::*, px, Bounds, ClickEvent, Context, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, Pixels, SharedString, Window,
};
use serde::{Deserialize, Serialize};

use crate::hover::HoverExt;
use crate::settings::{card, hsla, row, switch_row, text_button, AMBER, FILL, MUTED, TEXT, TRACK};
use gpui_m3::theme::type_scale::{self, TypeScaleExt};

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
    /// El estilo de las ventanas.
    pub windows: WindowStyle,
    /// Lo que tapan los fondos de las ventanas en Liquid Glass (0..1; 0,5 como Atic Code).
    pub glass_opacity: f32,
    /// El estilo y la opacidad ya son los mismos que los de Atic Code: la
    /// primera vez se toman de allá (`load`).
    pub linked: bool,
    /// El color de la piel de la pill y del notch.
    pub skin: Skin,
}

/// El color de la piel: el de Atic, uno liso o un degradado de izquierda a
/// derecha. Va siempre oscuro: el texto y los íconos encima son claros.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Skin {
    #[default]
    Atic,
    Solid { color: u32 },
    Gradient { from: u32, to: u32 },
}

/// Los que se ofrecen en Apariencia, con su nombre.
const SKINS: [(Skin, &str); 13] = [
    (Skin::Atic, "pill.appearance.skinAtic"),
    (Skin::Solid { color: 0x000000 }, "pill.appearance.skinBlack"),
    (Skin::Solid { color: 0x26272c }, "pill.appearance.skinGraphite"),
    (Skin::Solid { color: 0x14213d }, "pill.appearance.skinNight"),
    (Skin::Solid { color: 0x123524 }, "pill.appearance.skinForest"),
    (Skin::Solid { color: 0x3b1220 }, "pill.appearance.skinWine"),
    (Skin::Solid { color: 0x2a1640 }, "pill.appearance.skinPlum"),
    (Skin::Gradient { from: 0x0f0c29, to: 0x302b63 }, "pill.appearance.skinMidnight"),
    (Skin::Gradient { from: 0x0f2027, to: 0x2c5364 }, "pill.appearance.skinOcean"),
    (Skin::Gradient { from: 0x093028, to: 0x237a57 }, "pill.appearance.skinAurora"),
    (Skin::Gradient { from: 0x2b1055, to: 0x7d2a5c }, "pill.appearance.skinDusk"),
    (Skin::Gradient { from: 0x1f0a0a, to: 0x6a2614 }, "pill.appearance.skinEmber"),
    (Skin::Gradient { from: 0x0b0b0c, to: 0x3a3b40 }, "pill.appearance.skinSteel"),
];

impl Skin {
    /// Las dos puntas (iguales si es liso).
    pub fn ends(self) -> (u32, u32) {
        match self {
            Skin::Atic => (SKIN, SKIN),
            Skin::Solid { color } => (color, color),
            Skin::Gradient { from, to } => (from, to),
        }
    }

    /// El color que la representa donde no cabe un degradado (aros, fundidos
    /// sobre la piel): el del medio.
    pub fn mid(self) -> gpui::Hsla {
        let (a, b) = self.ends();
        let mix = |shift: u32| (((a >> shift) & 0xff) + ((b >> shift) & 0xff)) / 2;
        gpui::rgb((mix(16) << 16) | (mix(8) << 8) | mix(0)).into()
    }

    /// Para pintar: liso, o el degradado de izquierda a derecha.
    pub fn background(self, opacity: f32) -> gpui::Background {
        let (a, b) = self.ends();
        let (a, b): (gpui::Hsla, gpui::Hsla) = (gpui::rgb(a).into(), gpui::rgb(b).into());
        if a == b {
            return a.opacity(opacity).into();
        }
        gpui::linear_gradient(
            90.,
            gpui::linear_color_stop(a.opacity(opacity), 0.),
            gpui::linear_color_stop(b.opacity(opacity), 1.),
        )
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WindowStyle {
    /// La paleta de siempre de Atic.
    #[default]
    Atic,
    Expressive,
    Glass,
}

impl WindowStyle {
    /// El nombre (de marca, no se traduce) y la clave `i18n` de su ayuda.
    const ALL: [(WindowStyle, &'static str, &'static str); 3] = [
        (WindowStyle::Atic, "Atic", "pill.appearance.aticHint"),
        (WindowStyle::Expressive, "Material 3 Expressive", "pill.appearance.expressiveHint"),
        (WindowStyle::Glass, "Liquid Glass", "pill.appearance.glassHint"),
    ];

    /// El de las ventanas que va con un estilo de Atic Code.
    pub fn of_code(code: crate::code::style::Style) -> Self {
        match code {
            crate::code::style::Style::Formal => WindowStyle::Atic,
            crate::code::style::Style::Expressive => WindowStyle::Expressive,
            crate::code::style::Style::Glass => WindowStyle::Glass,
        }
    }

    /// El estilo de Atic Code que va con este: es una sola elección para
    /// todas las ventanas («Atic» es «Formal» en Atic Code).
    pub fn linked_code_style(self) -> crate::code::style::Style {
        self.code_style().unwrap_or(crate::code::style::Style::Formal)
    }

    /// El estilo de Atic Code que le presta sus tokens; `None` con «Atic».
    pub fn code_style(self) -> Option<crate::code::style::Style> {
        match self {
            WindowStyle::Atic => None,
            WindowStyle::Expressive => Some(crate::code::style::Style::Expressive),
            WindowStyle::Glass => Some(crate::code::style::Style::Glass),
        }
    }
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            glass: true,
            rest_tint: crate::GLASS_TINT,
            read_tint: crate::CONTENT_TINT,
            cover_hang: true,
            windows: WindowStyle::Atic,
            glass_opacity: 0.5,
            linked: false,
            skin: Skin::Atic,
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
    let bytes = file().and_then(|path| std::fs::read(path).ok());
    let mut look: Appearance = bytes.as_deref().and_then(|bytes| serde_json::from_slice(bytes).ok()).unwrap_or_default();
    // Una versión de prueba de macOS puso Liquid Glass en la pill y bajó su
    // tinte en reposo a 15 % (dejó `material` en el archivo): se repone.
    if bytes.as_deref().is_some_and(|bytes| String::from_utf8_lossy(bytes).contains("\"material\"")) {
        if look.rest_tint <= 0.15 {
            look.rest_tint = crate::GLASS_TINT;
        }
        save(&look);
    }
    // Antes el estilo de Atic Code y el de las otras ventanas se elegían
    // aparte: al juntarlos manda el de Atic Code, el que más se usaba.
    if !look.linked {
        let code = crate::code::config::Configs::load();
        look.windows = WindowStyle::of_code(code.style);
        look.glass_opacity = code.glass_opacity.unwrap_or(crate::code::style::DEFAULT_GLASS_OPACITY) as f32 / 100.0;
        look.linked = true;
        save(&look);
    }
    look.rest_tint = look.rest_tint.clamp(REST_RANGE.0, REST_RANGE.1);
    look.read_tint = look.read_tint.clamp(READ_RANGE.0, READ_RANGE.1);
    look.glass_opacity = look.glass_opacity.clamp(0.0, 1.0);
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
    Glass,
}

impl Knob {
    fn range(self) -> (f32, f32) {
        match self {
            Knob::Rest => REST_RANGE,
            Knob::Read => READ_RANGE,
            Knob::Glass => (0.0, 1.0),
        }
    }

    fn get(self, look: &Appearance) -> f32 {
        match self {
            Knob::Rest => look.rest_tint,
            Knob::Read => look.read_tint,
            Knob::Glass => look.glass_opacity,
        }
    }

    fn set(self, look: &mut Appearance, tint: f32) {
        match self {
            Knob::Rest => look.rest_tint = tint,
            Knob::Read => look.read_tint = tint,
            Knob::Glass => look.glass_opacity = tint,
        }
    }
}

#[derive(Default)]
pub struct AppearancePane {
    drag: Option<Knob>,
    rest_track: Rc<Cell<Bounds<Pixels>>>,
    read_track: Rc<Cell<Bounds<Pixels>>>,
    glass_track: Rc<Cell<Bounds<Pixels>>>,
}

impl AppearancePane {
    fn track(&self, knob: Knob) -> Rc<Cell<Bounds<Pixels>>> {
        match knob {
            Knob::Rest => self.rest_track.clone(),
            Knob::Read => self.read_track.clone(),
            Knob::Glass => self.glass_track.clone(),
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
        self.set_tint(knob, t, cx);
    }

    /// Pone el tinte `t` (0..1 de la barra) dentro del rango de la perilla.
    fn set_tint(&mut self, knob: Knob, t: f32, cx: &mut Context<Self>) {
        let (lo, hi) = knob.range();
        let mut look = current();
        knob.set(&mut look, t.clamp(lo, hi));
        apply(look, false);
        if knob == Knob::Glass {
            crate::theme::restyle();
        }
        cx.notify();
    }

    fn release(&mut self, cx: &mut Context<Self>) {
        if self.drag.take().is_some() {
            apply(current(), true);
            cx.notify();
        }
    }

    /// Una pill y un panel de muestra sobre un fondo claro (una página) y uno
    /// oscuro, con los tintes elegidos. No desenfoca: eso se ve en la pill. Sus colores
    /// son los de la pill y las páginas tal cual, sin pasar por el tema de los Ajustes
    /// (en modo claro, `hsla` volvería oscuro el texto sobre la piel oscura).
    fn preview(&self, look: &Appearance) -> impl IntoElement {
        let pill = crate::theme::raw;
        let half = |light: bool| {
            let (page, line) = if light { (0xf4f1ea, 0xc9c4b8) } else { (0x22303d, 0x3d5163) };
            let lines = [0.92, 0.78, 0.98, 0.64, 0.86, 0.72, 0.94].into_iter().map(move |w| {
                div()
                    .h(px(6.))
                    .w(gpui::relative(w))
                    .rounded(px(3.))
                    .bg(pill(line))
            });
            let hang = if look.cover_hang { look.read_tint } else { look.rest_tint };
            div()
                .flex_1()
                .min_w_0()
                .relative()
                .overflow_hidden()
                .bg(pill(page))
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
                                .bg(look.skin.background(look.rest_tint))
                                .flex()
                                .items_center()
                                .justify_center()
                                .gap(px(6.))
                                .text_size(px(10.))
                                .text_color(pill(TEXT))
                                .child(div().size(px(6.)).rounded_full().bg(pill(AMBER)))
                                .child("1"),
                        )
                        // Lo que cuelga: la letra.
                        .child(
                            div()
                                .mt(px(6.))
                                .px(px(10.))
                                .py(px(4.))
                                .rounded(px(10.))
                                .bg(look.skin.background(hang))
                                .text_size(px(10.))
                                .text_color(pill(TEXT))
                                .child(crate::i18n::t("pill.appearance.previewLyric")),
                        )
                        // Un panel abierto.
                        .child(
                            div()
                                .mt(px(8.))
                                .w(px(150.))
                                .p(px(10.))
                                .rounded(px(12.))
                                .bg(look.skin.background(look.read_tint))
                                .flex()
                                .flex_col()
                                .gap(px(3.))
                                .child(div().text_size(px(11.)).text_color(pill(TEXT)).child(crate::i18n::t("pill.appearance.previewPanel")))
                                .child(div().text_size(px(10.)).text_color(pill(MUTED)).child(crate::i18n::t("pill.appearance.previewText"))),
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

    fn slider(&self, knob: Knob, look: &Appearance, cx: &mut Context<Self>) -> gpui::AnyElement {
        let t = knob.get(look).clamp(0.0, 1.0);
        let store = self.track(knob);
        let width = f32::from(store.get().size.width);
        // Con estilo, el deslizador de gpui-m3 a lo ancho de la fila. Al arrastrar
        // queda `drag` puesto: se guarda al soltar, como con el de siempre.
        if crate::theme::styled().is_some() {
            let id: SharedString = format!("appearance-m3-{}", knob as u8).into();
            let pane = cx.entity().downgrade();
            return div()
                .relative()
                .w_full()
                .child(canvas(move |bounds, _, _| store.set(bounds), |_, _, _, _| {}).absolute().size_full())
                .child(
                    gpui_m3::Slider::new(id, t, 0.0..=1.0)
                        .width(px(if width > 0.0 { width } else { 240.0 }))
                        .on_change(move |value, _, cx| {
                            let _ = pane.update(cx, |pane, cx| {
                                pane.drag = Some(knob);
                                pane.set_tint(knob, value, cx);
                            });
                        }),
                )
                .into_any_element();
        }
        let fill = if width > SLIDER_H {
            SLIDER_H + (width - SLIDER_H) * t
        } else {
            SLIDER_H
        };
        let dragging = self.drag == Some(knob);
        let id = match knob {
            Knob::Rest => "appearance-rest",
            Knob::Read => "appearance-read",
            Knob::Glass => "appearance-glass-opacity",
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
                    .bg(if dragging { hsla(0xffffff) } else { hsla(FILL) }),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |pane, event: &MouseDownEvent, _, cx| {
                    pane.drag = Some(knob);
                    pane.drag_to(knob, event.position.x, cx);
                }),
            )
            .into_any_element()
    }
}

/// Las tres tarjetas para elegir el estilo de las ventanas, cada una con una
/// muestra pintada con sus colores; en Liquid Glass, también el deslizador de su
/// transparencia.
fn windows_style(look: Appearance, glass_slider: Option<gpui::AnyElement>) -> impl IntoElement {
    let light = crate::theme::is_light();
    // Con estilo, la elegida con el primario y el radio de las tarjetas del estilo.
    let styled = crate::theme::styled();
    let chosen = styled.as_ref().map_or_else(|| hsla(FILL), |t| t.accent);
    let tile_radius = styled.as_ref().map_or(12., |t| t.r_pop);
    let expressive = crate::theme::expressive();
    let tiles = WindowStyle::ALL.map(|(style, name, hint)| {
        let hint = crate::i18n::t(hint);
        let selected = look.windows == style;
        // Fondo, paneles, acento y radio de la muestra.
        let (bg, pane, accent, radius) = match style.code_style() {
            Some(code) => {
                let t = crate::code::style::tokens(code, light);
                (t.bg, t.raised, t.accent, t.r_chip.min(8.))
            }
            None => {
                let atic = |color| crate::theme::raw(crate::theme::resolve(color));
                (atic(crate::theme::WINDOW), atic(crate::theme::SURFACE_ON), atic(crate::theme::FILL), 4.)
            }
        };
        if let Some(t) = expressive {
            return expressive_style_tile(t, style, name, hint, selected, (bg, pane, accent, radius));
        }
        div()
            .id(SharedString::from(format!("windows-style-{style:?}")))
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .gap(px(8.))
            .p(px(8.))
            .rounded(px(tile_radius))
            .border_2()
            .border_color(if selected { chosen } else { hsla(TRACK) })
            .cursor_pointer()
            .on_click(move |_: &ClickEvent, window: &mut Window, _: &mut gpui::App| reveal(style, window))
            .child(
                div()
                    .h(px(52.))
                    .p(px(6.))
                    .flex()
                    .gap(px(5.))
                    .rounded(px(8.))
                    .bg(bg)
                    .child(div().w(px(18.)).rounded(px(radius)).bg(pane))
                    .child(div().flex_1().rounded(px(radius)).bg(pane))
                    .child(div().size(px(14.)).mt_auto().rounded_full().bg(accent)),
            )
            .child(div().text_size(px(13.)).text_color(hsla(TEXT)).child(name))
            .child(div().text_size(px(11.)).text_color(hsla(MUTED)).child(hint))
            .into_any_element()
    });
    card()
        .child(row(
            crate::i18n::t("pill.appearance.windowStyle"),
            crate::i18n::t("pill.appearance.windowStyleHint"),
            None,
            div().flex().gap(px(10.)).children(tiles),
        ))
        .when_some(glass_slider, |el, slider| {
            el.child(row(
                crate::i18n::t("pill.appearance.transparency"),
                crate::i18n::t("pill.appearance.transparencyHint"),
                Some(percent(1.0 - look.glass_opacity)),
                slider,
            ))
        })
}

/// Expressive: la tarjeta de un estilo como tarjeta seleccionable de M3. La
/// elegida va en el contenedor secundario, con contorno primario y esquinas más
/// redondas (28); las demás, con contorno fino, redondean sus esquinas al pasar
/// el cursor y las aprietan al hacer clic. Elegir otra saca a la ventana de
/// Expressive, así que el cambio de elegida no se anima aquí.
fn expressive_style_tile(
    t: crate::code::style::Tokens,
    style: WindowStyle,
    name: &'static str,
    hint: &'static str,
    selected: bool,
    (bg, pane, accent, radius): (gpui::Hsla, gpui::Hsla, gpui::Hsla, f32),
) -> gpui::AnyElement {
    use gpui_m3::corner;
    let (name_color, hint_color) = if selected { (t.on_accent_soft, t.on_accent_soft.opacity(0.8)) } else { (t.text, t.muted) };
    div()
        .id(SharedString::from(format!("windows-style-{style:?}")))
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .gap(px(8.))
        .map(|el| {
            // El mismo tamaño con contorno de 2 o de 1.
            if selected {
                el.p(px(8.)).border_2().border_color(t.accent).bg(t.accent_soft)
            } else {
                el.p(px(9.)).border_1().border_color(t.border)
            }
        })
        .cursor_pointer()
        .on_click(move |_: &ClickEvent, window: &mut Window, _: &mut gpui::App| reveal(style, window))
        .child(
            div()
                .h(px(52.))
                .p(px(6.))
                .flex()
                .gap(px(5.))
                .rounded(if selected { corner::LI } else { corner::S })
                .bg(bg)
                .child(div().w(px(18.)).rounded(px(radius)).bg(pane))
                .child(div().flex_1().rounded(px(radius)).bg(pane))
                .child(div().size(px(14.)).mt_auto().rounded_full().bg(accent)),
        )
        .child(div().text_color(name_color).type_style(type_scale::TITLE_SMALL).child(name))
        .child(div().text_color(hint_color).type_style(type_scale::BODY_SMALL).child(hint))
        .fx(SharedString::from(format!("windows-style-{style:?}-fx")), move |el, h| {
            if selected {
                el.rounded(corner::XL)
            } else {
                let r = f32::from(corner::M) + 8. * h.t - 4. * h.press;
                // `t.hover` en oscuro es el mismo color de la baldosa: no se vería.
                let hover = crate::settings::on_tile_bg(&t);
                el.rounded(px(r)).bg(h.mix(hover.opacity(0.), hover))
            }
        })
        .into_any_element()
}

/// Un cambio de estilo en curso: un círculo con el fondo nuevo crece desde el
/// puntero, como en Atic Code; cuando cubre la ventana se aplica el estilo y el
/// círculo se desvanece.
#[derive(Clone, Copy)]
struct Reveal {
    style: WindowStyle,
    origin: gpui::Point<Pixels>,
    radius: f32,
    color: gpui::Hsla,
    start: std::time::Instant,
    swapped: Option<std::time::Instant>,
}

static REVEAL: Mutex<Option<Reveal>> = Mutex::new(None);
/// Lo que tarda el círculo en desvanecerse con el estilo ya puesto.
const REVEAL_FADE: f32 = 0.2;

/// Cambia el estilo de las ventanas; con movimiento, con el círculo.
fn reveal(style: WindowStyle, window: &mut Window) {
    if current().windows == style {
        // Volviste al estilo de ahora con el círculo de otro creciendo: se
        // cancela (antes seguía y terminaba aplicando el que dejaste).
        if let Ok(mut running) = REVEAL.lock() {
            if running.is_some_and(|r| r.swapped.is_none()) {
                *running = None;
                window.refresh();
            }
        }
        return;
    }
    if crate::code::style::system_reduced_motion() {
        set_windows_style(style);
        window.refresh();
        return;
    }
    let light = crate::theme::is_light();
    // El fondo de la ventana con el estilo nuevo, opaco para que tape al de antes.
    let color = match style.code_style() {
        Some(code) => {
            let t = crate::code::style::tokens(code, light);
            if code == crate::code::style::Style::Glass { t.raised } else { t.pane }
        }
        None => crate::theme::raw(crate::theme::resolve(crate::theme::WINDOW)),
    };
    let origin = window.mouse_position();
    let size = window.viewport_size();
    let (x, y) = (f32::from(origin.x), f32::from(origin.y));
    let (w, h) = (f32::from(size.width), f32::from(size.height));
    let radius = x.max(w - x).hypot(y.max(h - y));
    if let Ok(mut running) = REVEAL.lock() {
        *running = Some(Reveal { style, origin, radius, color, start: std::time::Instant::now(), swapped: None });
    }
    window.refresh();
}

/// La opacidad del vidrio como la guarda Atic Code (0–100, 50 de fábrica):
/// la misma escala que `glass_opacity` (0..1, 0,5).
pub fn code_opacity(glass_opacity: f32) -> u8 {
    (glass_opacity.clamp(0.0, 1.0) * 100.0).round() as u8
}

/// Se eligió un estilo en Atic Code: las demás ventanas lo siguen.
pub fn follow_code_style(code: crate::code::style::Style) {
    let style = WindowStyle::of_code(code);
    if current().windows != style {
        set_windows_style(style);
    }
}

/// Se movió la opacidad del vidrio en Atic Code.
pub fn follow_code_opacity(opacity: u8) {
    let mut look = current();
    if code_opacity(look.glass_opacity) == opacity {
        return;
    }
    look.glass_opacity = opacity as f32 / 100.0;
    apply(look, true);
    crate::theme::restyle();
}

fn set_windows_style(style: WindowStyle) {
    let mut look = current();
    look.windows = style;
    apply(look, true);
    crate::theme::restyle();
}

/// La capa del círculo, al final de la raíz de los Ajustes; no pinta nada sin
/// un cambio en curso.
pub fn reveal_layer(window: &mut Window) -> gpui::AnyElement {
    let Ok(mut running) = REVEAL.lock() else {
        return div().into_any_element();
    };
    let Some(mut reveal) = *running else {
        return div().into_any_element();
    };
    let progress = gpui_m3::Spring::SPATIAL_SLOW.step(reveal.start.elapsed().as_secs_f32());
    let mut opacity = 1.0;
    match reveal.swapped {
        None if progress >= 0.999 => {
            set_windows_style(reveal.style);
            reveal.swapped = Some(std::time::Instant::now());
        }
        None => {}
        Some(at) => {
            let fade = at.elapsed().as_secs_f32() / REVEAL_FADE;
            if fade >= 1.0 {
                *running = None;
                return div().into_any_element();
            }
            opacity = 1.0 - fade;
        }
    }
    *running = Some(reveal);
    window.request_animation_frame();
    let r = (reveal.radius * progress.max(0.0)).max(0.0);
    let viewport = window.viewport_size();
    gpui::deferred(
        gpui::anchored().position(gpui::point(px(0.), px(0.))).child(
            div().w(viewport.width).h(viewport.height).overflow_hidden().child(
                div()
                    .absolute()
                    .left(reveal.origin.x - px(r))
                    .top(reveal.origin.y - px(r))
                    .size(px(r * 2.))
                    .rounded(px(r))
                    .bg(reveal.color.opacity(opacity)),
            ),
        ),
    )
    .with_priority(2)
    .into_any_element()
}

/// Una muestra por color: la elegida con un aro.
fn skin_swatches(chosen: Skin) -> impl IntoElement {
    let ring = crate::theme::styled().map_or_else(|| hsla(FILL), |t| t.accent);
    div().flex().flex_wrap().gap(px(8.)).children(SKINS.iter().enumerate().map(move |(i, &(skin, name))| {
        let selected = skin == chosen;
        div()
            .id(("appearance-skin", i))
            .size(px(30.))
            .p(px(3.))
            .rounded_full()
            .border_2()
            .border_color(if selected { ring } else { gpui::transparent_black() })
            .cursor_pointer()
            .tooltip(crate::hover::tip(crate::i18n::t(name)))
            .on_click(move |_: &ClickEvent, window: &mut Window, cx: &mut gpui::App| {
                let mut look = current();
                look.skin = skin;
                apply(look, true);
                window.refresh();
                // La pill no se redibuja sola en reposo.
                cx.refresh_windows();
            })
            .child(
                div()
                    .size_full()
                    .rounded_full()
                    .bg(skin.background(1.0))
                    .border_1()
                    .border_color(gpui::white().opacity(0.14)),
            )
    }))
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
                    if event.pressed_button != Some(MouseButton::Left) {
                        pane.release(cx);
                    } else if crate::theme::styled().is_none() {
                        // El deslizador de gpui-m3 se mueve solo; aquí solo se suelta.
                        pane.drag_to(knob, event.position.x, cx);
                    }
                }
            }))
            .on_mouse_up(MouseButton::Left, cx.listener(|pane, _: &MouseUpEvent, _, cx| pane.release(cx)))
            .on_mouse_up_out(MouseButton::Left, cx.listener(|pane, _: &MouseUpEvent, _, cx| pane.release(cx)))
            .child(windows_style(
                look,
                (look.windows == WindowStyle::Glass).then(|| self.slider(Knob::Glass, &look, cx).into_any_element()),
            ))
            .child(self.preview(&look))
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(hsla(MUTED))
                    .child(crate::i18n::t("pill.appearance.previewNote")),
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
                                crate::i18n::t("pill.appearance.frostedGlass"),
                                crate::i18n::t("pill.appearance.frostedGlassHint"),
                                look.glass,
                                toggle(|look| look.glass = !look.glass),
                            ))
                            .when(look.glass && !backdrop, |el| {
                                el.child(div().text_size(px(11.)).text_color(hsla(AMBER)).child(
                                    if cfg!(target_os = "macos") {
                                        crate::i18n::t("pill.appearance.reduceTransparencyMac")
                                    } else {
                                        crate::i18n::t("pill.appearance.reduceTransparencyWin")
                                    },
                                ))
                            }),
                    )
                    .child(row(
                        crate::i18n::t("pill.appearance.restTint"),
                        crate::i18n::t("pill.appearance.restTintHint"),
                        Some(percent(look.rest_tint)),
                        self.slider(Knob::Rest, &look, cx),
                    )),
            )
            .child(
                card()
                    .child(row(
                        crate::i18n::t("pill.appearance.readTint"),
                        crate::i18n::t("pill.appearance.readTintHint"),
                        Some(percent(look.read_tint)),
                        self.slider(Knob::Read, &look, cx),
                    ))
                    .child(switch_row(
                        "appearance-hang",
                        crate::i18n::t("pill.appearance.coverHang"),
                        crate::i18n::t("pill.appearance.coverHangHint"),
                        look.cover_hang,
                        toggle(|look| look.cover_hang = !look.cover_hang),
                    )),
            )
            .child(card().child(row(
                crate::i18n::t("pill.appearance.skin"),
                crate::i18n::t("pill.appearance.skinHint"),
                None,
                skin_swatches(look.skin),
            )))
            .child(
                div()
                    .flex()
                    .items_center()
                    .child(text_button("appearance-reset", crate::i18n::t("pill.customizeReset"), |_, window, _| {
                        apply(Appearance::default(), true);
                        window.refresh();
                    }))
                    .child(div().flex_1())
                    .child(div().text_size(px(11.)).text_color(hsla(MUTED)).child(crate::i18n::t("pill.appearance.autoSaved"))),
            )
    }
}
