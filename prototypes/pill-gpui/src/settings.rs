//! La ventana de Ajustes: una sección por tema, con la lista a la izquierda.
//!
//! Angosta (menos de `SIDEBAR_MIN_W`), la lista pasa a ser una fila de
//! pestañas arriba. Cada sección es su propia vista (`appearance.rs`) o una
//! función que arma su contenido; para agregar una, basta con sumarla a
//! `Section` y a `Section::ALL`.
//!
//! Se abre con presión larga sobre la cara de la pill, desde Personalizar o
//! con `PILL_OPEN=settings`.

use std::path::PathBuf;

use gpui::{
    div, prelude::*, px, size, svg, AnyView, App, Bounds, ClickEvent, Context, Entity, FontWeight, Hsla,
    SharedString, TitlebarOptions, Window, WindowBounds, WindowKind, WindowOptions,
};

use crate::appearance::AppearancePane;
use crate::dictation_settings::{self, DictationPane};
use crate::pill_settings::{self, ClipboardPane, PillPane};
use crate::hover::HoverExt;
use crate::space::chrome;

const WINDOW_W: f32 = 860.0;
const WINDOW_H: f32 = 720.0;
const TOP_H: f32 = 44.0;
const GUTTER: f32 = 22.0;
const SIDEBAR_W: f32 = 210.0;
/// Por debajo de este ancho la lista de secciones va arriba, en fila.
const SIDEBAR_MIN_W: f32 = 700.0;
/// El contenido no se estira más que esto en ventanas anchas.
const CONTENT_MAX_W: f32 = 640.0;

pub(crate) const WINDOW: u32 = 0x0f0f0e;
pub(crate) const SURFACE: u32 = 0x1d1d1b;
pub(crate) const SURFACE_ON: u32 = 0x2d2d2a;
pub(crate) const TRACK: u32 = 0x2d2d2a;
pub(crate) const FILL: u32 = 0xe9e9e2;
pub(crate) const TEXT: u32 = 0xf0f0ea;
pub(crate) const MUTED: u32 = 0x9a9a90;
pub(crate) const FAINT: u32 = 0x6a6a64;
pub(crate) const INK: u32 = 0x141413;
pub(crate) const AMBER: u32 = 0xe8b04b;
const SWITCH_OFF: u32 = 0x3a3a37;

pub(crate) fn hsla(color: u32) -> Hsla {
    gpui::rgb(color).into()
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Appearance,
    Pill,
    Clipboard,
    Dictation,
    Meetings,
    About,
}

impl Section {
    const ALL: [Section; 6] = [
        Section::Appearance,
        Section::Pill,
        Section::Clipboard,
        Section::Dictation,
        Section::Meetings,
        Section::About,
    ];

    fn label(self) -> &'static str {
        match self {
            Section::Appearance => "Apariencia",
            Section::Pill => "Pill",
            Section::Clipboard => "Portapapeles",
            Section::Dictation => "Dictado",
            Section::Meetings => "Reuniones",
            Section::About => "Acerca de",
        }
    }

    fn hint(self) -> &'static str {
        match self {
            Section::Appearance => "Cuánto se ve el vidrio y cuánto se tapa para leer.",
            Section::Pill => "Qué herramientas muestra y qué cuelga del notch.",
            Section::Clipboard => "Lo que copias, para pegarlo después.",
            Section::Dictation => "Se comparten con Atic. Cada cambio se guarda al tiro.",
            Section::Meetings => "Se comparten con Atic. Cada cambio se guarda al tiro.",
            Section::About => "Versión y dónde guarda sus datos.",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Section::Appearance => "icons/sparkles.svg",
            Section::Pill => "icons/layers.svg",
            Section::Clipboard => "icons/clipboard.svg",
            Section::Dictation => "icons/mic.svg",
            Section::Meetings => "icons/audio-lines.svg",
            Section::About => "icons/circle-dot.svg",
        }
    }

    fn id(self) -> &'static str {
        match self {
            Section::Appearance => "settings-appearance",
            Section::Pill => "settings-pill",
            Section::Clipboard => "settings-clipboard",
            Section::Dictation => "settings-dictation",
            Section::Meetings => "settings-meetings",
            Section::About => "settings-about",
        }
    }
}

/// Abre los Ajustes, o los trae al frente si ya están abiertos.
pub fn open(cx: &mut App) {
    let existing = cx.windows().into_iter().find_map(|w| w.downcast::<SettingsView>());
    if let Some(handle) = existing {
        if handle.update(cx, |_, window, _| window.activate_window()).is_ok() {
            return;
        }
    }
    let options = WindowOptions {
        titlebar: Some(TitlebarOptions {
            title: Some("Atic · Ajustes".into()),
            appears_transparent: true,
            ..Default::default()
        }),
        window_min_size: Some(size(px(420.), px(520.))),
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            size(px(WINDOW_W), px(WINDOW_H)),
            cx,
        ))),
        focus: true,
        show: true,
        kind: WindowKind::Normal,
        ..Default::default()
    };
    let opened = cx.open_window(options, |window, cx| {
        chrome::setup(window);
        cx.new(SettingsView::new)
    });
    if let Err(error) = opened {
        eprintln!("ajustes: no se pudo abrir la ventana: {error}");
    }
}

pub struct SettingsView {
    section: Section,
    appearance: Entity<AppearancePane>,
    /// Se crean al elegir la sección: Reuniones pide la lista de modelos a
    /// la red, y nada de eso hace falta para ver la Apariencia.
    pill: Entity<PillPane>,
    clipboard: Option<Entity<ClipboardPane>>,
    dictation: Option<Entity<DictationPane>>,
    meetings: Option<AnyView>,
}

impl SettingsView {
    fn new(cx: &mut Context<Self>) -> Self {
        Self {
            section: Section::Appearance,
            appearance: cx.new(|_| AppearancePane::default()),
            pill: cx.new(|_| PillPane),
            clipboard: None,
            dictation: None,
            meetings: None,
        }
    }

    fn select(&mut self, section: Section, cx: &mut Context<Self>) {
        match section {
            Section::Clipboard if self.clipboard.is_none() => self.clipboard = pill_settings::clipboard_pane(cx),
            Section::Dictation if self.dictation.is_none() => self.dictation = dictation_settings::pane(cx),
            Section::Meetings if self.meetings.is_none() => self.meetings = crate::meetings::settings_pane(cx),
            _ => {}
        }
        self.section = section;
        cx.notify();
    }

    fn top_bar(&self, maximized: bool) -> impl IntoElement {
        div()
            .h(px(TOP_H))
            .flex_none()
            .pl(px(GUTTER - 6.0))
            .flex()
            .items_center()
            .gap(px(10.))
            .child(chrome::logo(24.0))
            .child(div().text_size(px(14.)).font_weight(FontWeight::SEMIBOLD).child("Ajustes"))
            .child(chrome::drag(TOP_H))
            .child(chrome::controls(maximized, TOP_H))
    }

    /// Una entrada de la lista (o de la fila de pestañas, si `compact`).
    fn nav_item(&self, section: Section, compact: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = self.section == section;
        let (rest, over) = if selected {
            (hsla(SURFACE_ON), hsla(SURFACE_ON))
        } else {
            (hsla(WINDOW), hsla(SURFACE))
        };
        div()
            .id(section.id())
            .flex()
            .items_center()
            .gap(px(10.))
            .px(px(12.))
            .py(px(if compact { 6. } else { 8. }))
            .rounded(px(10.))
            .cursor_pointer()
            .text_size(px(13.))
            .text_color(hsla(if selected { TEXT } else { MUTED }))
            .when(compact, |el| el.flex_none())
            .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.select(section, cx)))
            .child(
                svg()
                    .path(section.icon())
                    .size(px(15.))
                    .flex_none()
                    .text_color(hsla(if selected { TEXT } else { FAINT })),
            )
            .child(section.label())
            .hover_bg(SharedString::from(format!("{}-fx", section.id())), rest, over)
    }

    fn body(&self) -> gpui::AnyElement {
        match self.section {
            Section::Appearance => self.appearance.clone().into_any_element(),
            Section::Pill => self.pill.clone().into_any_element(),
            Section::Clipboard => match &self.clipboard {
                Some(pane) => pane.clone().into_any_element(),
                None => missing_data().into_any_element(),
            },
            Section::Dictation => match &self.dictation {
                Some(pane) => pane.clone().into_any_element(),
                None => missing_data().into_any_element(),
            },
            Section::Meetings => match &self.meetings {
                Some(pane) => pane.clone().into_any_element(),
                None => missing_data().into_any_element(),
            },
            Section::About => about().into_any_element(),
        }
    }
}

impl Render for SettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let wide = f32::from(window.viewport_size().width) >= SIDEBAR_MIN_W;
        let section = self.section;

        let content = div()
            .id("settings-content")
            .flex_1()
            .min_w_0()
            .overflow_y_scroll()
            .px(px(GUTTER))
            .pt(px(if wide { 10. } else { 4. }))
            .pb(px(GUTTER))
            .child(
                div()
                    .w_full()
                    .max_w(px(CONTENT_MAX_W))
                    .flex()
                    .flex_col()
                    .gap(px(16.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .child(
                                div()
                                    .text_size(px(20.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(section.label()),
                            )
                            .child(div().text_size(px(12.)).text_color(hsla(MUTED)).child(section.hint())),
                    )
                    .child(self.body()),
            );

        let nav: Vec<_> = Section::ALL.iter().map(|&s| self.nav_item(s, !wide, cx)).collect();
        let main = if wide {
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .child(
                    div()
                        .w(px(SIDEBAR_W))
                        .flex_none()
                        .px(px(12.))
                        .pt(px(10.))
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .children(nav),
                )
                .child(content)
        } else {
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .child(
                    div()
                        .id("settings-tabs")
                        .flex_none()
                        .px(px(GUTTER - 8.0))
                        .pb(px(10.))
                        .flex()
                        .gap(px(4.))
                        .overflow_x_scroll()
                        .children(nav),
                )
                .child(content)
        };

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(hsla(WINDOW))
            .font_family("Segoe UI")
            .text_color(hsla(TEXT))
            .child(self.top_bar(window.is_maximized()))
            .child(main)
    }
}

// --- Piezas comunes de las secciones -----------------------------------------------

/// Un grupo de ajustes sobre su propio fondo.
pub(crate) fn card() -> gpui::Div {
    div()
        .p(px(18.))
        .rounded(px(14.))
        .bg(hsla(SURFACE))
        .flex()
        .flex_col()
        .gap(px(18.))
}

/// Título, valor a la derecha, el control y una línea de ayuda.
pub(crate) fn row(
    title: &'static str,
    hint: &'static str,
    value: Option<SharedString>,
    control: impl IntoElement,
) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap(px(8.))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(12.))
                .child(div().flex_1().min_w_0().text_size(px(13.)).text_color(hsla(TEXT)).child(title))
                .children(value.map(|value| div().flex_none().text_size(px(12.)).text_color(hsla(MUTED)).child(value))),
        )
        .child(control)
        .child(div().text_size(px(11.)).text_color(hsla(FAINT)).child(hint))
}

/// Título y ayuda a la izquierda, el interruptor a la derecha.
pub(crate) fn switch_row(
    id: &'static str,
    title: &'static str,
    hint: &'static str,
    on: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .gap(px(16.))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(3.))
                .child(div().text_size(px(13.)).text_color(hsla(TEXT)).child(title))
                .child(div().text_size(px(11.)).text_color(hsla(FAINT)).child(hint)),
        )
        .child(
            div()
                .id(id)
                .w(px(36.))
                .h(px(22.))
                .flex_none()
                .p(px(3.))
                .flex()
                .when(on, |el| el.justify_end())
                .rounded_full()
                .cursor_pointer()
                .on_click(on_click)
                .child(div().size(px(16.)).rounded_full().bg(hsla(if on { INK } else { MUTED })))
                .fx(id, move |el, h| {
                    let (rest, over) = if on { (0xe9e9e2, 0xffffff) } else { (SWITCH_OFF, 0x46463f) };
                    el.bg(h.mix(hsla(rest), hsla(over)))
                }),
        )
}

/// Un botón de texto discreto.
pub(crate) fn text_button(
    id: &'static str,
    label: &'static str,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .flex_none()
        .text_size(px(12.))
        .text_color(hsla(TEXT))
        .cursor_pointer()
        .rounded_full()
        .px(px(12.))
        .py(px(5.))
        .on_click(on_click)
        .child(label)
        .hover_bg(SharedString::from(format!("{id}-fx")), hsla(TEXT).opacity(0.08), hsla(TEXT).opacity(0.14))
}

/// Sin la carpeta de Atic no hay `config.json` que editar.
fn missing_data() -> impl IntoElement {
    card().child(
        div()
            .text_size(px(13.))
            .text_color(hsla(MUTED))
            .child("No se encontró la carpeta de datos de Atic."),
    )
}

// --- Acerca de ----------------------------------------------------------------------

fn data_dir() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join("atic-gpui"))
}

fn about() -> impl IntoElement {
    let folder: SharedString = data_dir()
        .map(|dir| dir.display().to_string())
        .unwrap_or_else(|| "—".into())
        .into();
    card()
        .child(row(
            "Versión",
            "La pill nativa de Atic (prototipo GPUI).",
            Some(env!("CARGO_PKG_VERSION").into()),
            div(),
        ))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(12.))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(3.))
                        .child(div().text_size(px(13.)).text_color(hsla(TEXT)).child("Carpeta de datos"))
                        .child(div().text_size(px(11.)).text_color(hsla(FAINT)).child(folder)),
                )
                .child(text_button("settings-open-folder", "Abrir", |_, _, _| {
                    if let Some(dir) = data_dir() {
                        let _ = std::fs::create_dir_all(&dir);
                        let _ = std::process::Command::new("explorer").arg(dir).spawn();
                    }
                })),
        )
}
