//! La ventana de Reuniones: las grabaciones de Atic, con su resumen y su
//! transcripción. Es una ventana normal (como el Espacio), no el notch: es para
//! leer con calma.
//!
//! Primera etapa: solo lectura de los datos de Atic (ver `data`). Grabar,
//! transcribir, resumir, el reproductor y la edición vienen después.

mod data;
mod actions;
mod export;
mod pipeline;
pub(crate) mod settings;
mod player;
mod player_view;
mod stretch;
mod find;
mod speakers;
mod chime;
mod live;
mod motion;
mod recorder;
mod detect;
mod pill;

pub use recorder::{stopwatch, studio, Stage, Studio};
pub use pill::{peek_body, peek_height, Ink, MEET_TOOL_HINT};

/// El rojo de grabar: el punto del tab y el de la ventana.
pub const RECORD_RED: u32 = RED;
mod row_peek;
mod search;
mod import;
mod summary;
mod text;

use atic_core::{Recording, RecordingStatus};
use chrono::Local;
use gpui::{
    actions, div, point, prelude::*, px, size, App, Bounds, ClickEvent, Context, ElementId,
    FocusHandle, FontWeight, Hsla, KeyBinding, ScrollHandle, SharedString, Window,
    WindowHandle,
};

use crate::hover::HoverExt;
use crate::space::chrome;
use data::Source;
use summary::{Block as SummaryBlock, Kind, Section};
use text::Block;

actions!(meetings, [SelectPrev, SelectNext, ToggleTab, Reload]);

const KEY_CONTEXT: &str = "Meetings";

pub fn bind_keys(cx: &mut App) {
    let context = Some(KEY_CONTEXT);
    cx.bind_keys([
        KeyBinding::new("up", SelectPrev, context),
        KeyBinding::new("down", SelectNext, context),
        KeyBinding::new("tab", ToggleTab, context),
        KeyBinding::new("f5", Reload, context),
    ]);
    recorder::bind_keys(cx);
    actions::bind_keys(cx);
    player_view::bind_keys(cx);
    settings::bind_keys(cx);
    search::bind_keys(cx);
}

// Medidas y colores: los del Mando (ver `space/mando.rs`), planos y sin bordes.
const TOP_H: f32 = 48.0;
const GUTTER: f32 = 16.0;
const LIST_W: f32 = 320.0;
const R_PANEL: f32 = 20.0;
const R_CARD: f32 = 16.0;
const R_ITEM: f32 = 14.0;

const WINDOW: u32 = 0x0f0f0e;
const SURFACE: u32 = 0x1d1d1b;
const SURFACE_HOVER: u32 = 0x252523;
const SURFACE_ON: u32 = 0x2d2d2a;
const ITEM: u32 = 0x262624;
const TEXT: u32 = 0xf0f0ea;
const BODY: u32 = 0xd9d9d1;
const MUTED: u32 = 0x9a9a90;
const FAINT: u32 = 0x6a6a64;
const INK: u32 = 0x141413;
const AMBER: u32 = 0xe8b04b;
const GREEN: u32 = 0x6cc48a;
const BLUE: u32 = 0x7fa8f0;
const RED: u32 = 0xe5705f;
const LILAC: u32 = 0xc4a3f0;

fn hsla(color: u32) -> Hsla {
    gpui::rgb(color).into()
}

/// Los ajustes de Reuniones, para la sección de la ventana de Ajustes.
/// `None` si no se encuentra la carpeta de datos de Atic.
pub fn settings_pane(cx: &mut App) -> Option<gpui::AnyView> {
    let paths = data::Paths::resolve().ok()?;
    Some(cx.new(|cx| settings::SettingsView::new_embedded(paths, cx)).into())
}

/// Muestra la ventana de Reuniones; la abre si no está.
pub fn show(cx: &mut App) {
    let existing = cx.windows().into_iter().find_map(|w| w.downcast::<MeetingsView>());
    if let Some(handle) = existing {
        let alive = handle
            .update(cx, |view, window, cx| {
                window.activate_window();
                view.reload(cx);
            })
            .is_ok();
        if alive {
            return;
        }
    }
    if let Err(error) = open_window(cx) {
        eprintln!("reuniones: no se pudo abrir la ventana: {error}");
    }
}

/// Muestra la ventana con esa reunión elegida.
pub fn show_meeting(id: String, cx: &mut App) {
    show(cx);
    let Some(handle) = cx.windows().into_iter().find_map(|w| w.downcast::<MeetingsView>()) else {
        return;
    };
    let _ = handle.update(cx, |view, _, cx| {
        view.reload(cx);
        if let Some(ix) = view.items.iter().position(|r| r.id == id) {
            view.select(ix, cx);
        }
    });
}

pub fn open_window(cx: &mut App) -> anyhow::Result<WindowHandle<MeetingsView>> {
    let options = gpui::WindowOptions {
        titlebar: Some(gpui::TitlebarOptions {
            title: Some("Atic · Reuniones".into()),
            appears_transparent: true,
            ..Default::default()
        }),
        window_min_size: Some(size(px(760.), px(520.))),
        window_bounds: Some(gpui::WindowBounds::Windowed(Bounds::centered(
            None,
            size(px(1180.), px(780.)),
            cx,
        ))),
        focus: true,
        show: true,
        kind: gpui::WindowKind::Normal,
        ..Default::default()
    };
    Ok(cx.open_window(options, |window, cx| {
        chrome::setup(window);
        cx.new(|cx| MeetingsView::new(window, cx))
    })?)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Summary,
    Transcript,
}

/// Lo que se muestra de la reunión elegida, leído al elegirla.
struct Detail {
    id: String,
    blocks: Vec<Block>,
    has_transcript: bool,
    sections: Vec<Section>,
    summary_meta: Option<String>,
    subject: Option<String>,
    error: Option<String>,
}

pub struct MeetingsView {
    source: Option<Source>,
    items: Vec<Recording>,
    error: Option<String>,
    selected: Option<usize>,
    tab: Tab,
    detail: Option<Detail>,
    focus: FocusHandle,
    detail_scroll: ScrollHandle,
    /// La grabadora de la app (`recorder::Studio`), compartida con la pill.
    studio: gpui::Entity<recorder::Studio>,
    player: player_view::PlayerUi,
    ops: actions::Ops,
    /// Los ajustes abiertos en lugar de la lista y el detalle.
    settings: Option<(gpui::Entity<settings::SettingsView>, gpui::Subscription)>,
    /// El hover de las filas: acciones rápidas y vistazo (ver `row_peek`).
    row_peeks: row_peek::RowPeeks,
    /// Buscar en la transcripción (Ctrl+F) y renombrar hablantes.
    find: find::Find,
    speakers: speakers::Speakers,
    /// Buscar en la lista (Ctrl+K) e importar audio (ver `search`, `import`).
    search: search::Search,
    imports: import::Imports,
}

impl MeetingsView {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        window.focus(&focus);
        // Al volver a la ventana se relee: Atic puede haber grabado o resumido
        // algo mientras tanto.
        cx.observe_window_activation(window, |view, window, cx| {
            if window.is_window_active() {
                view.reload(cx);
            }
        })
        .detach();
        let (source, error) = match Source::open() {
            Ok(source) => (Some(source), None),
            Err(error) => (None, Some(format!("No se encontraron los datos de Atic: {error}"))),
        };
        let mut view = Self {
            source,
            items: Vec::new(),
            error,
            selected: None,
            tab: Tab::Summary,
            detail: None,
            focus,
            detail_scroll: ScrollHandle::new(),
            studio: recorder::studio(cx),
            player: Default::default(),
            ops: Default::default(),
            settings: None,
            row_peeks: Default::default(),
            find: Default::default(),
            speakers: Default::default(),
            search: Default::default(),
            imports: Default::default(),
        };
        view.install_recorder(cx);
        view.install_search(cx);
        view.reload(cx);
        view
    }

    fn reload(&mut self, cx: &mut Context<Self>) {
        let Some(source) = &self.source else {
            return;
        };
        let previous = self.selected.and_then(|ix| self.items.get(ix)).map(|r| r.id.clone());
        self.row_peeks.clear();
        match source.list() {
            Ok(mut items) => {
                // La eliminada a la espera de «Deshacer» no se ve.
                items.retain(|r| !self.ops.hides(&r.id));
                self.items = items;
                self.error = None;
            }
            Err(error) => self.error = Some(format!("No se pudo leer la lista: {error}")),
        }
        self.refresh_silent();
        let index = previous
            .and_then(|id| self.items.iter().position(|r| r.id == id))
            .or((!self.items.is_empty()).then_some(0));
        // Misma reunión: se relee sin mover la pestaña ni el scroll.
        let keep_place = index.is_some() && index == self.selected;
        self.load_detail(index, !keep_place);
        self.refresh_search(cx);
        cx.notify();
    }

    fn select(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.selected == Some(index) || index >= self.items.len() {
            return;
        }
        self.load_detail(Some(index), true);
        cx.notify();
    }

    fn load_detail(&mut self, index: Option<usize>, fresh: bool) {
        self.selected = index;
        let (Some(source), Some(rec)) = (&self.source, index.and_then(|ix| self.items.get(ix)))
        else {
            self.detail = None;
            return;
        };
        let mut error = None;
        let transcript = source.transcript(&rec.id).unwrap_or_else(|e| {
            error = Some(format!("No se pudo leer la transcripción: {e}"));
            None
        });
        let summary = source.summary(&rec.id).unwrap_or_else(|e| {
            error = Some(format!("No se pudo leer el resumen: {e}"));
            None
        });
        let sections = summary
            .as_ref()
            .map(|s| summary::parse(&s.body, "Resumen"))
            .unwrap_or_default();
        if fresh {
            self.tab = if sections.is_empty() { Tab::Transcript } else { Tab::Summary };
            self.detail_scroll.set_offset(point(px(0.), px(0.)));
        }
        self.detail = Some(Detail {
            id: rec.id.clone(),
            has_transcript: transcript.is_some(),
            blocks: transcript.map(|t| text::blocks(&t.segments)).unwrap_or_default(),
            summary_meta: summary.as_ref().map(|s| {
                let when = s.created_at.with_timezone(&Local);
                format!(
                    "{} · {} · {}",
                    template_label(&s.template),
                    backend_label(&s.backend),
                    when.format("%d-%m-%Y %H:%M")
                )
            }),
            subject: summary.and_then(|s| s.subject).filter(|s| !s.trim().is_empty()),
            sections,
            error,
        });
    }

    /// ↑/↓ recorren lo que se ve (con una búsqueda, solo lo que coincide).
    fn step(&mut self, delta: isize, cx: &mut Context<Self>) {
        let rows = self.visible_rows();
        if rows.is_empty() {
            return;
        }
        let last = rows.len() as isize - 1;
        let next = match self.selected.and_then(|ix| rows.iter().position(|&r| r == ix)) {
            Some(at) => (at as isize + delta).clamp(0, last),
            None => 0,
        };
        let ix = rows[next as usize];
        self.select(ix, cx);
        self.reveal_row(ix, delta < 0);
    }

    fn set_tab(&mut self, tab: Tab, cx: &mut Context<Self>) {
        if self.tab != tab {
            self.tab = tab;
            self.detail_scroll.set_offset(point(px(0.), px(0.)));
            cx.notify();
        }
    }

    fn toggle_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.settings.take().is_some() {
            window.focus(&self.focus);
            cx.notify();
            return;
        }
        let Some(paths) = self.source.as_ref().and_then(|s| s.paths()).cloned() else {
            return;
        };
        let view = cx.new(|cx| settings::SettingsView::new(paths, cx));
        let subscription = cx.subscribe_in(&view, window, |this, _, event, window, cx| match event {
            settings::SettingsEvent::Close => {
                this.settings = None;
                window.focus(&this.focus);
                cx.notify();
            }
        });
        window.focus(&gpui::Focusable::focus_handle(view.read(cx), cx));
        self.settings = Some((view, subscription));
        cx.notify();
    }

    fn open_folder(&self) {
        let Some(path) = self
            .detail
            .as_ref()
            .and_then(|d| self.source.as_ref()?.folder(&d.id))
        else {
            return;
        };
        if let Err(error) = std::process::Command::new("explorer").arg(path).spawn() {
            eprintln!("reuniones: no se pudo abrir la carpeta: {error}");
        }
    }
}

impl Render for MeetingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let maximized = window.is_maximized();
        self.sync_player(cx);
        self.sync_menu_focus(window, cx);
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(hsla(WINDOW))
            .text_color(hsla(TEXT))
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus)
            .on_action(cx.listener(|v, _: &SelectPrev, _, cx| v.step(-1, cx)))
            .on_action(cx.listener(|v, _: &SelectNext, _, cx| v.step(1, cx)))
            .on_action(cx.listener(|v, _: &ToggleTab, _, cx| {
                let next = if v.tab == Tab::Summary { Tab::Transcript } else { Tab::Summary };
                v.set_tab(next, cx)
            }))
            .on_action(cx.listener(|v, _: &Reload, _, cx| v.reload(cx)))
            .map(|el| Self::player_actions(el, cx))
            .map(|el| Self::delete_actions(el, cx))
            .map(|el| Self::shortcut_actions(el, cx))
            .map(|el| Self::search_actions(el, cx))
            .map(|el| Self::drop_target(el, cx))
            .on_action(cx.listener(|v, _: &recorder::ToggleRecording, _, cx| v.toggle_recording(cx)))
            .child(self.top_bar(maximized, cx))
            .child(match &self.settings {
                // Los ajustes ocupan el cuerpo entero; la barra (y grabar) siguen.
                Some((view, _)) => div().flex_1().min_h_0().child(view.clone()).into_any_element(),
                None => div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .gap(px(12.))
                    .px(px(GUTTER))
                    .pb(px(GUTTER))
                    .relative()
                    .child(self.list(cx))
                    .child(self.detail_panel(cx))
                    .children(self.undo_toast(cx))
                    .into_any_element(),
            })
    }
}

impl MeetingsView {
    fn top_bar(&self, maximized: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let count = match self.items.len() {
            0 => String::new(),
            1 => "1 grabación".into(),
            n => format!("{n} grabaciones"),
        };
        div()
            .h(px(TOP_H))
            .flex_none()
            .pl(px(GUTTER))
            .flex()
            .items_center()
            .gap(px(12.))
            .child(chrome::logo(28.0))
            .child(
                div()
                    .text_size(px(14.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Reuniones"),
            )
            .child(div().text_size(px(12.)).text_color(hsla(MUTED)).child(count))
            .child(chrome::drag(TOP_H))
            .children(self.record_button(cx))
            .when(self.source.as_ref().is_some_and(|s| s.paths().is_some()), |el| {
                el.child(crate::hover::round_button(
                    "meetings-settings",
                    "icons/settings-2.svg",
                    "Ajustes de reuniones · Ctrl+,",
                    self.settings.is_some(),
                    hsla(TEXT),
                    hsla(MUTED),
                    cx.listener(|v, _: &ClickEvent, window, cx| v.toggle_settings(window, cx)),
                ))
            })
            .child(div().w(px(4.)))
            .child(chrome::controls(maximized, TOP_H))
    }

    fn list(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let today = Local::now().date_naive();
        let panel = div()
            .w(px(LIST_W))
            .flex_none()
            .flex()
            .flex_col()
            .relative()
            .rounded(px(R_PANEL))
            .bg(hsla(SURFACE))
            // Encima de los días: lo que se está grabando.
            .children(self.live_card(cx));
        if self.items.is_empty() && !self.imports.busy() {
            let panel = match &self.error {
                Some(error) => panel.child(empty("No se pudieron leer las reuniones".into(), error.clone())),
                None => panel
                    .child(self.invite(cx))
                    .children(self.import_rows(cx))
                    .children(self.import_link(cx)),
            };
            return panel.children(self.drop_veil(cx));
        }
        let panel = panel.children(self.search_bar(cx)).children(self.import_rows(cx));
        if self.visible_rows().is_empty() {
            let panel = if self.searching() { panel.child(self.no_results(cx)) } else { panel };
            return panel.children(self.drop_veil(cx));
        }
        // Encabezados y filas son hijos directos del scroll: sus medidas dicen
        // qué día va pegado arriba (ver `search::sticky_day`).
        let mut column = div()
            .id("meetings-list")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.search.scroll)
            .flex()
            .flex_col()
            .gap(px(2.))
            .px(px(8.))
            .pt(px(8.));
        let mut days = Vec::new();
        for (child, slot) in self.list_slots().into_iter().enumerate() {
            column = match slot {
                search::Slot::Day(day) => {
                    let label = text::day_label(day, today);
                    days.push((child, label.clone()));
                    column.child(search::day_header(label, child == 0))
                }
                search::Slot::Row(ix) => column.child(self.row(ix, &self.items[ix], cx)),
            };
        }
        panel
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    // El corte del scroll queda dentro de las esquinas del panel;
                    // el encabezado fijo, empujado, sale por arriba sin tapar el
                    // buscador.
                    .mb(px(8.))
                    .relative()
                    .overflow_hidden()
                    .child(column)
                    .children(self.sticky_day(&days)),
            )
            .children(self.drop_veil(cx))
    }

    fn row(&self, ix: usize, rec: &Recording, cx: &mut Context<Self>) -> impl IntoElement {
        let on = self.selected == Some(ix);
        let (status, color) = status_label(rec.status, self.ops.is_silent(&rec.id));
        let local = rec.started_at.with_timezone(&Local);
        let meta = div()
            .flex()
            .items_center()
            .gap(px(6.))
            .text_size(px(12.))
            .text_color(hsla(MUTED))
            .child(format!("{} · {}", local.format("%H:%M"), text::duration(rec.duration_secs)))
            .child(div().flex_1())
            .child(dot(color))
            .child(status);
        let row = div()
            .id(ElementId::NamedInteger("meeting".into(), ix as u64))
            .px(px(12.))
            .py(px(9.))
            .rounded(px(R_ITEM))
            .flex()
            .flex_col()
            .gap(px(3.))
            .when(!on, |el| el.cursor_pointer())
            .on_click(cx.listener(move |v, _: &ClickEvent, window, cx| {
                // Elegida con el mouse, el teclado vuelve a la lista.
                v.leave_search(window, cx);
                v.select(ix, cx)
            }))
            .child(
                div()
                    .truncate()
                    .text_size(px(13.))
                    .font_weight(FontWeight::MEDIUM)
                    .child(SharedString::from(rec.title.clone())),
            )
            // Coincide por lo que se dijo o se resumió: dónde aparece.
            .children(self.snippet_line(ix));
        // El fondo, la línea de abajo que se cruza con acciones y el vistazo.
        self.row_reveal(ix, rec, on, row, meta, cx)
    }

    fn detail_panel(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let panel = div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .rounded(px(R_PANEL))
            .bg(hsla(SURFACE));
        let (Some(rec), Some(detail)) = (self.selected.and_then(|ix| self.items.get(ix)), &self.detail)
        else {
            if self.items.is_empty() {
                return panel;
            }
            return panel.child(empty("Elige una reunión".into(), "Con ↑ y ↓ también se cambia.".into()));
        };
        let today = Local::now().date_naive();
        let local = rec.started_at.with_timezone(&Local);
        let (status, color) = status_label(rec.status, self.ops.is_silent(&rec.id));
        let header = div()
            .flex_none()
            .px(px(24.))
            .pt(px(22.))
            .pb(px(14.))
            .flex()
            .items_start()
            .gap(px(16.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(self.title_view(rec, cx))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .text_size(px(13.))
                            .text_color(hsla(MUTED))
                            .child(format!(
                                "{} · {} · {}",
                                text::full_date(local.date_naive(), today),
                                local.format("%H:%M"),
                                text::duration(rec.duration_secs)
                            ))
                            .child(div().w(px(4.)))
                            .child(dot(color))
                            .child(status),
                    )
                    .children(self.notice_line(rec, cx)),
            )
            .child(self.header_actions(rec, detail, cx));

        let transcript_label: SharedString = if detail.blocks.is_empty() {
            "Transcripción".into()
        } else {
            format!("Transcripción · {}", detail.blocks.len()).into()
        };
        // La píldora encendida se desliza al cambiar de pestaña (ver `motion`).
        let active = if self.tab == Tab::Summary { 0 } else { 1 };
        let sliding = self.ops.slide.observe(active);
        let tab_el = |ix: usize, id, label, target, cx: &mut Context<Self>| match &sliding {
            Some(s) => s.ink(ix, hsla(INK), hsla(MUTED), tab(id, label, false, true, target, cx)),
            None => tab(id, label, ix == active, false, target, cx).into_any_element(),
        };
        let tabs = div().flex_none().px(px(24.)).pb(px(14.)).flex().child(
            div()
                .relative()
                .flex()
                .items_center()
                .gap(px(2.))
                .p(px(3.))
                .rounded(px(15.))
                .bg(hsla(0x161615))
                .on_children_prepainted(self.ops.slide.recorder())
                .children(sliding.as_ref().map(|s| s.pill(3., hsla(0xe9e9e2), 13.)))
                .child(tab_el(0, "tab-summary", "Resumen".into(), Tab::Summary, cx))
                .child(tab_el(1, "tab-transcript", transcript_label, Tab::Transcript, cx)),
        );

        let body = match self.tab {
            Tab::Summary => self.summary_body(rec, detail, cx),
            Tab::Transcript => self.transcript_body(rec, detail, cx),
        };
        // Al cambiar de reunión o de pestaña el contenido entra fundiéndose;
        // el id solo cambia con ellas (no con el streaming ni el reproductor).
        let header = motion::fade_in(ElementId::Name(format!("head-{}", rec.id).into()), 0., header);
        let body = motion::fade_in(ElementId::Name(format!("body-{}-{active}", rec.id).into()), motion::RISE, body);
        panel.child(header).child(tabs)
        // Buscar (Ctrl+F): fija arriba del contenido, no se va con el scroll.
        .children(self.find_bar(cx))
        .child(
            div()
                .id("meeting-detail")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .track_scroll(&self.detail_scroll)
                .on_scroll_wheel(self.player_wheel(cx))
                // El corte del scroll queda sobre el panel, no contra su borde.
                .mb(px(14.))
                .px(px(24.))
                .pb(px(10.))
                .when_some(detail.error.clone(), |el, error| {
                    el.child(div().pb(px(12.)).text_size(px(13.)).text_color(hsla(RED)).child(error))
                })
                .child(body),
        )
        // El reproductor, al pie: no se va con el scroll.
        .children(self.player_bar(cx))
    }

    /// Generar, leer, editar y mandar el resumen: ver `actions`.
    fn summary_body(&self, rec: &Recording, detail: &Detail, cx: &mut Context<Self>) -> gpui::AnyElement {
        self.summary_view(rec, detail, cx)
    }

    fn transcript_body(&self, rec: &Recording, detail: &Detail, cx: &mut Context<Self>) -> gpui::AnyElement {
        if detail.blocks.is_empty() {
            // Transcribir y reintentar están en la cabecera: aquí solo se dice dónde.
            let (title, hint) = match rec.status {
                RecordingStatus::Transcribing => ("Transcribiendo…", "Aparecerá aquí al terminar."),
                RecordingStatus::Error => ("La transcripción falló", "Usa «Transcribir» para intentarlo de nuevo."),
                _ if detail.has_transcript => ("No se escuchó a nadie", "Revisa en Ajustes qué micrófono se usa y que no esté silenciado."),
                _ => ("Sin transcribir todavía", "Usa «Transcribir», arriba a la derecha."),
            };
            return empty(title.into(), hint.into()).into_any_element();
        }
        let mut column = div().flex().flex_col().gap(px(4.));
        for (ix, block) in detail.blocks.iter().enumerate() {
            let content = div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(3.))
                // Un clic le pone nombre al hablante (ver `speakers`).
                .child(self.speaker_label(ix, block, cx))
                .child(
                    div()
                        .text_size(px(14.))
                        .line_height(px(22.))
                        .text_color(hsla(BODY))
                        // Con lo buscado resaltado (ver `find`).
                        .child(self.block_text(ix, block)),
                );
            // La hora y el resaltado del que suena son del reproductor.
            column = column.child(self.transcript_block(ix, block, content, cx));
        }
        self.find_measure(column).into_any_element()
    }
}

/// `live`: es la sección que se está escribiendo (su punto late).
fn section_card(section: &Section, lead: bool, live: bool) -> impl IntoElement {
    let marker = match section.kind {
        Kind::Summary => AMBER,
        Kind::Decisions => GREEN,
        Kind::Tasks => BLUE,
        Kind::Topics => MUTED,
        Kind::General => FAINT,
    };
    let mut card = div()
        .p(px(18.))
        .rounded(px(R_CARD))
        .bg(hsla(if lead { SURFACE_ON } else { ITEM }))
        .flex()
        .flex_col()
        .gap(px(10.))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(9.))
                .child(if live {
                    actions::pulse_dot("summary-live-dot", marker).into_any_element()
                } else {
                    div().size(px(7.)).flex_none().rounded_full().bg(hsla(marker)).into_any_element()
                })
                .child(
                    div()
                        .text_size(px(14.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(SharedString::from(section.title.clone())),
                ),
        );
    if section.blocks.is_empty() {
        card = card.child(div().text_size(px(13.)).text_color(hsla(FAINT)).child("Sin contenido."));
    }
    for block in &section.blocks {
        card = card.child(match block {
            SummaryBlock::Paragraph(text) => body_text(text.clone()).into_any_element(),
            SummaryBlock::List { ordered, items } => {
                let mut list = div().flex().flex_col().gap(px(6.));
                for (n, item) in items.iter().enumerate() {
                    let marker = match (item.checked, ordered) {
                        (Some(done), _) => checkbox(done).into_any_element(),
                        (None, true) => div()
                            .text_size(px(14.))
                            .text_color(hsla(MUTED))
                            .child(format!("{}.", n + 1))
                            .into_any_element(),
                        (None, false) => div()
                            .mt(px(9.))
                            .size(px(5.))
                            .rounded_full()
                            .bg(hsla(MUTED))
                            .into_any_element(),
                    };
                    let done = item.checked == Some(true);
                    list = list.child(
                        div()
                            .flex()
                            .gap(px(10.))
                            .child(div().w(px(18.)).flex_none().flex().justify_center().child(marker))
                            .child(
                                body_text(item.text.clone())
                                    .when(done, |el| el.text_color(hsla(MUTED)).line_through()),
                            ),
                    );
                }
                list.into_any_element()
            }
        });
    }
    card
}

fn body_text(text: String) -> gpui::Div {
    div()
        .flex_1()
        .min_w_0()
        .text_size(px(14.))
        .line_height(px(22.))
        .text_color(hsla(BODY))
        .child(SharedString::from(text))
}

fn checkbox(done: bool) -> impl IntoElement {
    div()
        .mt(px(4.))
        .size(px(14.))
        .rounded(px(4.))
        .flex()
        .items_center()
        .justify_center()
        .bg(hsla(if done { GREEN } else { 0x3a3a37 }))
        .when(done, |el| {
            el.text_size(px(10.)).font_weight(FontWeight::BOLD).text_color(hsla(INK)).child("✓")
        })
}

fn dot(color: u32) -> impl IntoElement {
    div().size(px(6.)).flex_none().rounded_full().bg(hsla(color))
}

fn empty(title: String, hint: String) -> impl IntoElement {
    div()
        .flex_1()
        .py(px(48.))
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(6.))
        .child(div().text_size(px(14.)).font_weight(FontWeight::MEDIUM).child(title))
        .child(div().text_size(px(13.)).text_color(hsla(MUTED)).child(hint))
}

fn tab(
    id: &'static str,
    label: SharedString,
    on: bool,
    // Mientras la píldora pasa por debajo: sin fondo propio, y el color del
    // texto lo pone quien la envuelve (ver `motion::Sliding::ink`).
    sliding: bool,
    target: Tab,
    cx: &mut Context<MeetingsView>,
) -> impl IntoElement {
    div()
        .id(id)
        .h(px(26.))
        .px(px(14.))
        .flex()
        .items_center()
        .rounded(px(13.))
        .text_size(px(12.))
        .font_weight(FontWeight::MEDIUM)
        .when(!sliding, |el| el.text_color(hsla(if on { INK } else { MUTED })))
        .when(on, |el| el.bg(hsla(0xe9e9e2)))
        .when(!on, |el| el.cursor_pointer())
        .on_click(cx.listener(move |v, _: &ClickEvent, _, cx| v.set_tab(target, cx)))
        .child(label)
        .fx(id, move |el, h| if on || sliding { el } else { el.text_color(h.mix(hsla(MUTED), hsla(TEXT))) })
}

/// `silent`: transcrita, pero nadie habló (ver `pipeline::is_silent`).
fn status_label(status: RecordingStatus, silent: bool) -> (&'static str, u32) {
    match status {
        RecordingStatus::Transcribed if silent => ("Sin voz", FAINT),
        RecordingStatus::Recorded => ("Sin transcribir", FAINT),
        RecordingStatus::Transcribing => ("Transcribiendo", AMBER),
        RecordingStatus::Transcribed => ("Transcrita", BLUE),
        RecordingStatus::Summarizing => ("Resumiendo", AMBER),
        RecordingStatus::Summarized => ("Con resumen", GREEN),
        RecordingStatus::Error => ("Con error", RED),
    }
}

fn template_label(template: &str) -> &str {
    match template {
        "summary_key_points" => "Puntos clave",
        "executive_minutes" => "Acta ejecutiva",
        "action_items" => "Tareas",
        "followup_email" => "Correo de seguimiento",
        other => other,
    }
}

fn backend_label(backend: &str) -> &str {
    match backend {
        "claude" => "Claude",
        "ollama" => "Ollama",
        "openai" => "OpenAI",
        "openrouter" => "OpenRouter",
        "groq" => "Groq",
        "manual" => "editado a mano",
        other => other,
    }
}
