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
use crate::general_settings::{CapturesPane, GeneralPane, LauncherPane};
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

// Los colores son los del tema (`theme`): cambian con él.
pub(crate) use crate::theme::{AMBER, FAINT, FILL, INK, MUTED, SURFACE, SURFACE_ON, TEXT, WINDOW};
pub(crate) const TRACK: u32 = crate::theme::SURFACE_ON;
const SWITCH_OFF: u32 = 0x3a3a37;

pub(crate) fn hsla(color: u32) -> Hsla {
    crate::theme::hsla(color)
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Section {
    General,
    Appearance,
    Pill,
    Clipboard,
    Dictation,
    Meetings,
    Captures,
    Launcher,
    Shortcuts,
    Agents,
    Phone,
    About,
}

impl Section {
    const ALL: [Section; 12] = [
        Section::General,
        Section::Appearance,
        Section::Pill,
        Section::Clipboard,
        Section::Dictation,
        Section::Meetings,
        Section::Captures,
        Section::Launcher,
        Section::Shortcuts,
        Section::Agents,
        Section::Phone,
        Section::About,
    ];

    fn label(self) -> &'static str {
        match self {
            Section::General => crate::i18n::t("settings.nav.general"),
            Section::Captures => crate::i18n::t("settings.nav.captures"),
            Section::Launcher => crate::i18n::t("settings.nav.launcher"),
            Section::Shortcuts => crate::i18n::t("settings.nav.shortcuts"),
            Section::Agents => crate::i18n::t("settings.nav.agents"),
            Section::Phone => crate::i18n::t("settings.nav.phone"),
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
            Section::General => crate::i18n::t("pill.settings.generalHint"),
            Section::Captures => crate::i18n::t("pill.settings.capturesHint"),
            Section::Launcher => crate::i18n::t("pill.settings.launcherHint"),
            Section::Shortcuts => crate::i18n::t("settings.shortcuts.title"),
            Section::Agents => crate::i18n::t("settings.agents.aticMcpTitle"),
            Section::Phone => crate::i18n::t("pill.phone.sectionHint"),
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
            Section::General => "icons/settings-2.svg",
            Section::Captures => "icons/crop.svg",
            Section::Launcher => "icons/search.svg",
            Section::Shortcuts => "icons/type.svg",
            Section::Agents => "icons/square-terminal.svg",
            Section::Phone => "icons/laptop.svg",
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
            Section::General => "settings-general",
            Section::Captures => "settings-captures",
            Section::Launcher => "settings-launcher",
            Section::Shortcuts => "settings-shortcuts",
            Section::Agents => "settings-agents",
            Section::Phone => "settings-phone",
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
        if handle
            .update(cx, |_, window, _| {
                window.activate_window();
                crate::win::bring_to_front(window);
            })
            .is_ok()
        {
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
    match opened {
        Ok(handle) => {
            let _ = handle.update(cx, |_, window, _| crate::win::bring_to_front(window));
        }
        Err(error) => eprintln!("ajustes: no se pudo abrir la ventana: {error}"),
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
    general: Option<Entity<GeneralPane>>,
    captures: Option<Entity<CapturesPane>>,
    launcher: Option<Entity<LauncherPane>>,
    shortcuts: Option<Entity<crate::shortcuts_settings::ShortcutsPane>>,
    agents: Option<Entity<crate::agents_settings::AgentsPane>>,
    phone: Option<Entity<crate::phone_settings::PhonePane>>,
    /// El material del sistema puesto detrás (`theme::sync_backdrop`).
    backdrop: Option<bool>,
}

impl SettingsView {
    fn new(cx: &mut Context<Self>) -> Self {
        Self {
            section: Section::General,
            appearance: cx.new(|_| AppearancePane::default()),
            pill: cx.new(|_| PillPane::new()),
            clipboard: None,
            dictation: None,
            meetings: None,
            general: crate::general_settings::general_pane(cx),
            captures: None,
            launcher: None,
            shortcuts: None,
            agents: None,
            phone: None,
            backdrop: None,
        }
    }

    fn select(&mut self, section: Section, cx: &mut Context<Self>) {
        match section {
            Section::Clipboard if self.clipboard.is_none() => self.clipboard = pill_settings::clipboard_pane(cx),
            Section::Dictation if self.dictation.is_none() => self.dictation = dictation_settings::pane(cx),
            Section::Meetings if self.meetings.is_none() => self.meetings = crate::meetings::settings_pane(cx),
            Section::General if self.general.is_none() => self.general = crate::general_settings::general_pane(cx),
            Section::Captures if self.captures.is_none() => self.captures = crate::general_settings::captures_pane(cx),
            Section::Launcher if self.launcher.is_none() => self.launcher = crate::general_settings::launcher_pane(cx),
            Section::Shortcuts if self.shortcuts.is_none() => self.shortcuts = crate::shortcuts_settings::shortcuts_pane(cx),
            Section::Agents if self.agents.is_none() => self.agents = Some(crate::agents_settings::agents_pane(cx)),
            Section::Phone if self.phone.is_none() => self.phone = Some(crate::phone_settings::phone_pane(cx)),
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
        let styled = crate::theme::styled();
        let (rest, over) = match styled {
            // M3: la elegida en el contenedor secundario, como la barra de navegación.
            Some(t) if selected => (t.sel, t.sel),
            Some(t) => (t.hover.opacity(0.), t.hover),
            None if selected => (hsla(SURFACE_ON), hsla(SURFACE_ON)),
            None => (hsla(WINDOW), hsla(SURFACE)),
        };
        let glass_edge = styled.filter(|t| selected && t.style == crate::code::style::Style::Glass).map(|t| t.highlight);
        div()
            .id(section.id())
            .flex()
            .items_center()
            .gap(px(10.))
            .px(px(if styled.is_some() { 16. } else { 12. }))
            .py(px(if compact { 6. } else if styled.is_some() { 10. } else { 8. }))
            .map(|el| if styled.is_some() { el.rounded_full() } else { el.rounded(px(10.)) })
            .when_some(glass_edge, |el, edge| el.border_1().border_color(edge))
            .when(selected && styled.is_some(), |el| el.font_weight(FontWeight::SEMIBOLD))
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

    fn body(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
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
            Section::General => pane_or_missing(&self.general),
            Section::Captures => pane_or_missing(&self.captures),
            Section::Launcher => pane_or_missing(&self.launcher),
            Section::Shortcuts => pane_or_missing(&self.shortcuts),
            Section::Agents => pane_or_missing(&self.agents),
            Section::Phone => pane_or_missing(&self.phone),
            Section::About => about(cx).into_any_element(),
        }
    }
}

impl Render for SettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::theme::sync_backdrop(window, &mut self.backdrop);
        let m3_scope = crate::theme::scope_m3(cx);
        let styled = crate::theme::styled();
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
                                    .text_size(px(title_size(styled.as_ref())))
                                    .font_weight(title_weight(styled.as_ref()))
                                    .child(section.label()),
                            )
                            .child(div().text_size(px(12.)).text_color(hsla(MUTED)).child(section.hint())),
                    )
                    .child(self.body(cx))
                    .map(|el| enter(el, section, styled.as_ref())),
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
            .bg(crate::theme::window_bg())
            .font_family(crate::theme::font())
            .text_color(hsla(TEXT))
            .child(self.top_bar(window.is_maximized()))
            .child(main)
            .child(crate::appearance::reveal_layer(window))
            .children(m3_scope)
    }
}

// --- Piezas comunes de las secciones -----------------------------------------------

/// Un grupo de ajustes sobre su propio fondo.
pub(crate) fn card() -> gpui::Div {
    div()
        .p(px(18.))
        .rounded(px(crate::theme::card_radius(14.)))
        .bg(hsla(SURFACE))
        .when_some(crate::theme::styled().filter(|t| t.style == crate::code::style::Style::Glass), |el, t| {
            el.border_1().border_color(t.highlight)
        })
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
        .child(switch(id, on, on_click))
}

/// Un interruptor: el de gpui-m3 en M3 y en el vidrio, el de siempre en «Atic».
pub(crate) fn switch(
    id: &'static str,
    on: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> gpui::AnyElement {
    if crate::theme::styled().is_some() {
        return div()
            .id(id)
            .flex_none()
            .cursor_pointer()
            .on_click(on_click)
            .child(gpui_m3::Switch::new(SharedString::from(format!("{id}-m3")), on))
            .into_any_element();
    }
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
        })
        .into_any_element()
}

/// El tamaño del título de la sección: el titular de M3, más marcado en el vidrio.
fn title_size(styled: Option<&crate::code::style::Tokens>) -> f32 {
    match styled.map(|t| t.style) {
        Some(crate::code::style::Style::Expressive) => 26.,
        Some(_) => 22.,
        None => 20.,
    }
}

fn title_weight(styled: Option<&crate::code::style::Tokens>) -> FontWeight {
    match styled.map(|t| t.style) {
        Some(crate::code::style::Style::Expressive) => FontWeight::NORMAL,
        Some(_) => FontWeight::BOLD,
        None => FontWeight::SEMIBOLD,
    }
}

/// La entrada del contenido al cambiar de sección. M3 sube con la curva enfatizada;
/// el vidrio aparece desde un poco más chico; «Atic» cambia de una vez, como siempre.
fn enter(el: gpui::Div, section: Section, styled: Option<&crate::code::style::Tokens>) -> gpui::AnyElement {
    use gpui::{Animation, AnimationExt};
    use std::time::Duration;
    let Some(style) = styled.map(|t| t.style) else {
        return el.into_any_element();
    };
    if crate::code::style::system_reduced_motion() {
        return el.into_any_element();
    }
    let id = SharedString::from(format!("settings-enter-{}-{style:?}", section.id()));
    if style == crate::code::style::Style::Expressive {
        el.with_animation(
            id,
            Animation::new(Duration::from_millis(450)).with_easing(gpui_m3::motion::cubic_bezier(0.05, 0.7, 0.1, 1.)),
            |el, p| el.opacity(p).mt(px((1. - p) * 24.)),
        )
        .into_any_element()
    } else {
        el.with_animation(
            id,
            Animation::new(Duration::from_millis(320)).with_easing(gpui_m3::motion::cubic_bezier(0.2, 0., 0., 1.)),
            |el, p| el.opacity(p).mt(px((1. - p) * 8.)),
        )
        .into_any_element()
    }
}

/// Un botón de texto discreto.
pub(crate) fn text_button(
    id: &'static str,
    label: &'static str,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> gpui::AnyElement {
    if crate::theme::styled().is_some() {
        return gpui_m3::Button::new(id, label).tonal().on_click(on_click).into_any_element();
    }
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
        .into_any_element()
}

fn pane_or_missing<V: Render>(pane: &Option<Entity<V>>) -> gpui::AnyElement {
    match pane {
        Some(pane) => pane.clone().into_any_element(),
        None => missing_data().into_any_element(),
    }
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

/// Buscar e instalar actualizaciones (`updater.rs`).
fn updates(cx: &mut Context<SettingsView>) -> impl IntoElement {
    use crate::i18n::{t, tf};
    use crate::updater::{self, Status};
    let status = updater::status();
    let value: Option<SharedString> = match &status {
        Status::Checking => Some(t("about.checking").into()),
        Status::UpToDate => Some(t("about.upToDate").into()),
        Status::Available(update) => Some(tf("about.availableTitle", &[("version", &update.version)]).into()),
        Status::Installing => Some(t("about.installingBody").into()),
        Status::Failed(_) => Some(t("about.checkFailed").into()),
        Status::Unknown => None,
    };
    let control = match status {
        Status::Available(update) => div().flex().child(text_button(
            "settings-update-install",
            t("pill.settings.installUpdate"),
            cx.listener(move |_, _: &ClickEvent, _, cx| {
                let update = update.clone();
                cx.spawn(async move |view, cx| {
                    let restart = cx.background_spawn(async move { updater::install(&update) }).await;
                    let _ = view.update(cx, |_, cx| {
                        if restart {
                            cx.quit();
                        } else {
                            cx.notify();
                        }
                    });
                })
                .detach();
                cx.notify();
            }),
        )),
        Status::Checking | Status::Installing => div(),
        _ => div().flex().child(text_button(
            "settings-update-check",
            t("about.check"),
            cx.listener(|_, _: &ClickEvent, _, cx| {
                cx.spawn(async move |view, cx| {
                    cx.background_spawn(async { updater::check_now() }).await;
                    let _ = view.update(cx, |_, cx| cx.notify());
                })
                .detach();
                cx.notify();
            }),
        )),
    };
    row(t("about.updates"), t("pill.settings.updatesHint"), value, control)
}

fn data_dir() -> Option<PathBuf> {
    crate::paths::data_dir()
}

fn about(cx: &mut Context<SettingsView>) -> impl IntoElement {
    let folder: SharedString = data_dir()
        .map(|dir| dir.display().to_string())
        .unwrap_or_else(|| "—".into())
        .into();
    card()
        .child(row(
            "Versión",
            "La pill nativa de Atic.",
            Some(crate::updater::VERSION.into()),
            div(),
        ))
        .child(updates(cx))
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
