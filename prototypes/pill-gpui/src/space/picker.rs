//! El selector de carpetas de Atic Code, dentro de la ventana: sin el
//! diálogo de Windows.
//!
//! A la izquierda, lugares (inicio, documentos, las recientes de los agentes y
//! las unidades). A la derecha, las subcarpetas de donde se está. Un clic en
//! el nombre entra; el círculo la marca. Se pueden marcar varias, también de
//! carpetas distintas, y se confirman juntas. Lo que se escribe filtra la
//! lista; las flechas y Enter navegan, Retroceso sube, Esc cierra.

use std::path::{Path, PathBuf};
use std::time::Duration;

use gpui::{ease_in_out, div, prelude::*, px, svg, Animation, AnimationExt, ClickEvent, Context, Div, FontWeight, MouseButton, MouseDownEvent, SharedString};

use super::console::hsla;
use super::workspaces::{same, short_name};
use super::SpaceView;

const TEXT: u32 = 0xf0f0ea;
const MUTED: u32 = 0x9a9a90;
const FAINT: u32 = 0x6a6a64;
const INK: u32 = 0x141413;
const MODAL: u32 = 0x1b1b19;
const SIDE: u32 = 0x161615;
const ROW_HOVER: u32 = 0x262624;
const ROW_ON: u32 = 0x2e2e2b;
const ACCENT: u32 = 0x6cc48a;
/// Una carpeta con miles de subcarpetas no se lista entera.
const MAX_ENTRIES: usize = 800;

/// Para qué se eligen las carpetas.
#[derive(Clone, Copy)]
pub enum Purpose {
    NewWorkspace,
    AddTo(u64),
}

struct Entry {
    path: PathBuf,
    name: String,
    hidden: bool,
}

pub struct Picker {
    pub purpose: Purpose,
    /// Dónde se está; `None` es la portada con los lugares.
    dir: Option<PathBuf>,
    entries: Vec<Entry>,
    filter: String,
    /// La fila resaltada con las flechas, entre las que pasan el filtro.
    cursor: usize,
    pub selected: Vec<PathBuf>,
    places: Vec<(&'static str, PathBuf)>,
    recent: Vec<PathBuf>,
    drives: Vec<PathBuf>,
    error: Option<String>,
}

fn places() -> Vec<(&'static str, PathBuf)> {
    let mut out: Vec<(&'static str, PathBuf)> = Vec::new();
    let home = crate::agents::home();
    let onedrive = std::env::var_os("OneDriveCommercial").or_else(|| std::env::var_os("OneDrive")).map(PathBuf::from);
    let mut push = |label: &'static str, path: Option<PathBuf>| {
        if let Some(path) = path.filter(|p| p.is_dir()) {
            if !out.iter().any(|(_, known)| same(known, &path)) {
                out.push((label, path));
            }
        }
    };
    push("Inicio", home.clone());
    push("Escritorio", onedrive.as_ref().map(|o| o.join("Escritorio")));
    push("Escritorio", home.as_ref().map(|h| h.join("Desktop")));
    push("Documentos", onedrive.as_ref().map(|o| o.join("Documentos")));
    push("Documentos", home.as_ref().map(|h| h.join("Documents")));
    push("GitHub", onedrive.as_ref().map(|o| o.join("Documentos").join("GitHub")));
    push("GitHub", home.as_ref().map(|h| h.join("Documents").join("GitHub")));
    push("Repos", home.as_ref().map(|h| h.join("source").join("repos")));
    push("OneDrive", onedrive);
    out
}

fn drives() -> Vec<PathBuf> {
    (b'C'..=b'Z').map(|letter| PathBuf::from(format!("{}:\\", letter as char))).filter(|p| p.is_dir()).collect()
}

fn list_dir(dir: &Path) -> Result<Vec<Entry>, String> {
    let read = std::fs::read_dir(dir).map_err(|error| error.to_string())?;
    let mut entries: Vec<Entry> = read
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            Entry { hidden: name.starts_with('.') || name.starts_with('$'), path: e.path(), name }
        })
        .collect();
    // Las ocultas al final; el resto por nombre, sin mirar mayúsculas.
    entries.sort_by(|a, b| a.hidden.cmp(&b.hidden).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    entries.truncate(MAX_ENTRIES);
    Ok(entries)
}

impl Picker {
    pub fn new(purpose: Purpose) -> Self {
        Self {
            purpose,
            dir: None,
            entries: Vec::new(),
            filter: String::new(),
            cursor: 0,
            selected: Vec::new(),
            places: places(),
            recent: crate::agents::recent_folders(8),
            drives: drives(),
            error: None,
        }
    }

    fn go(&mut self, dir: Option<PathBuf>) {
        self.filter.clear();
        self.cursor = 0;
        self.error = None;
        match &dir {
            Some(path) => match list_dir(path) {
                Ok(entries) => self.entries = entries,
                Err(error) => {
                    self.error = Some(error);
                    return;
                }
            },
            None => self.entries.clear(),
        }
        self.dir = dir;
    }

    fn up(&mut self) {
        let parent = self.dir.as_ref().and_then(|d| d.parent().map(Path::to_path_buf));
        self.go(parent);
    }

    fn visible(&self) -> Vec<&Entry> {
        let filter = self.filter.to_lowercase();
        self.entries.iter().filter(|e| filter.is_empty() || e.name.to_lowercase().contains(&filter)).collect()
    }

    fn toggle(&mut self, path: &Path) {
        match self.selected.iter().position(|s| same(s, path)) {
            Some(index) => {
                self.selected.remove(index);
            }
            None => self.selected.push(path.to_path_buf()),
        }
    }

    fn is_selected(&self, path: &Path) -> bool {
        self.selected.iter().any(|s| same(s, path))
    }

    pub fn type_text(&mut self, text: &str) {
        // El espacio marca la carpeta resaltada (`key`), no filtra.
        if self.dir.is_some() && text != " " {
            self.filter.push_str(text);
            self.cursor = 0;
        }
    }

    /// Las teclas del selector. Dice si la atendió.
    pub fn key(&mut self, key: &str, control: bool) -> KeyResult {
        match key {
            "escape" if !self.filter.is_empty() => self.filter.clear(),
            "escape" => return KeyResult::Close,
            "enter" if control => return KeyResult::Confirm,
            "backspace" => {
                if self.filter.pop().is_none() {
                    self.up();
                }
            }
            "down" => self.cursor = (self.cursor + 1).min(self.visible().len().saturating_sub(1)),
            "up" => self.cursor = self.cursor.saturating_sub(1),
            "enter" | "right" => {
                if let Some(path) = self.visible().get(self.cursor).map(|e| e.path.clone()) {
                    self.go(Some(path));
                }
            }
            "left" => self.up(),
            "space" => {
                if let Some(path) = self.visible().get(self.cursor).map(|e| e.path.clone()) {
                    self.toggle(&path);
                }
            }
            _ => return KeyResult::Ignored,
        }
        KeyResult::Handled
    }
}

pub enum KeyResult {
    Handled,
    Ignored,
    Close,
    Confirm,
}

impl SpaceView {
    pub(super) fn open_picker(&mut self, purpose: Purpose, cx: &mut Context<Self>) {
        let mut picker = Picker::new(purpose);
        // Empieza donde están las carpetas del espacio, si tiene.
        let start = match purpose {
            Purpose::AddTo(id) => self.spaces.get(id).and_then(|s| s.main()).and_then(|f| f.parent()).map(Path::to_path_buf),
            Purpose::NewWorkspace => None,
        };
        picker.go(start);
        self.picker = Some(picker);
        cx.notify();
    }

    pub(super) fn confirm_picker(&mut self, cx: &mut Context<Self>) {
        let Some(picker) = self.picker.take() else {
            return;
        };
        if picker.selected.is_empty() {
            self.picker = Some(picker);
            return;
        }
        match picker.purpose {
            Purpose::NewWorkspace => {
                self.spaces.create(picker.selected);
            }
            Purpose::AddTo(id) => self.spaces.add_folders(id, picker.selected),
        }
        cx.notify();
    }
}

fn update<F: Fn(&mut Picker) + 'static>(
    cx: &mut Context<SpaceView>,
    f: F,
) -> Box<dyn Fn(&ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static> {
    Box::new(cx.listener(move |view: &mut SpaceView, _: &ClickEvent, _, cx| {
        if let Some(picker) = &mut view.picker {
            f(picker);
            cx.notify();
        }
    }))
}

fn side_item(id: SharedString, icon: &'static str, label: String, on: bool) -> gpui::Stateful<Div> {
    div()
        .id(id)
        .h(px(28.))
        .px(px(8.))
        .flex()
        .flex_none()
        .items_center()
        .gap(px(8.))
        .rounded(px(8.))
        .text_size(px(12.))
        .text_color(hsla(if on { TEXT } else { MUTED }))
        .when(on, |el| el.bg(hsla(ROW_ON)))
        .hover(|el| el.bg(hsla(ROW_HOVER)))
        .cursor_pointer()
        .child(svg().path(icon).size(px(13.)).flex_none().text_color(hsla(MUTED)))
        .child(div().truncate().child(label))
}

fn heading(text: &'static str) -> Div {
    div().px(px(8.)).pt(px(10.)).pb(px(2.)).text_size(px(10.5)).font_weight(FontWeight::SEMIBOLD).text_color(hsla(FAINT)).child(text)
}

fn check(on: bool) -> Div {
    div()
        .size(px(18.))
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .rounded(px(9.))
        .bg(hsla(if on { ACCENT } else { 0x33332f }))
        .when(on, |el| el.child(svg().path("icons/check.svg").size(px(12.)).text_color(hsla(INK))))
}

/// El selector encima de todo: un velo que cierra al hacer clic afuera.
pub fn render(view: &SpaceView, vw: f32, vh: f32, cx: &mut Context<SpaceView>) -> Option<impl IntoElement> {
    let picker = view.picker.as_ref()?;
    let (title, action) = match picker.purpose {
        Purpose::NewWorkspace => ("Nuevo espacio", "Crear espacio"),
        Purpose::AddTo(_) => ("Agregar carpetas", "Agregar"),
    };
    let w = 760f32.min(vw - 40.0);
    let h = 560f32.min(vh - 40.0);
    let visible = picker.visible();
    let current = picker.dir.clone();

    // El camino hasta donde se está, cada tramo se puede pinchar.
    let mut crumbs: Vec<(String, PathBuf)> = Vec::new();
    if let Some(dir) = &current {
        let mut walk = Some(dir.as_path());
        while let Some(path) = walk {
            let label = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.display().to_string());
            crumbs.push((label, path.to_path_buf()));
            walk = path.parent();
        }
        crumbs.reverse();
    }

    let mut side = div()
        .id("picker-side")
        .w(px(190.))
        .h_full()
        .flex_none()
        .p(px(8.))
        .flex()
        .flex_col()
        .gap(px(1.))
        .bg(hsla(SIDE))
        .rounded_l(px(16.))
        .overflow_y_scroll()
        .child(heading("LUGARES"));
    for (index, (label, path)) in picker.places.iter().enumerate() {
        let on = current.as_ref().is_some_and(|c| same(c, path));
        let target = path.clone();
        side = side.child(
            side_item(format!("pk-place-{index}").into(), "icons/folder.svg", label.to_string(), on)
                .on_click(update(cx, move |p| p.go(Some(target.clone())))),
        );
    }
    if !picker.recent.is_empty() {
        side = side.child(heading("RECIENTES"));
        for (index, path) in picker.recent.iter().enumerate() {
            let on = current.as_ref().is_some_and(|c| same(c, path));
            let target = path.clone();
            side = side.child(
                side_item(format!("pk-recent-{index}").into(), "icons/rotate-ccw.svg", short_name(path), on)
                    .tooltip(crate::hover::tip_text(path.display().to_string().into()))
                    .on_click(update(cx, move |p| p.go(Some(target.clone())))),
            );
        }
    }
    side = side.child(heading("UNIDADES"));
    for (index, path) in picker.drives.iter().enumerate() {
        let target = path.clone();
        side = side.child(
            side_item(format!("pk-drive-{index}").into(), "icons/monitor.svg", path.display().to_string(), false)
                .on_click(update(cx, move |p| p.go(Some(target.clone())))),
        );
    }

    let here_selected = current.as_ref().is_some_and(|c| picker.is_selected(c));
    let path_bar = div()
        .flex()
        .items_center()
        .gap(px(4.))
        .min_w(px(0.))
        .flex_1()
        .overflow_hidden()
        .text_size(px(12.))
        .children(crumbs.iter().enumerate().map(|(index, (label, path))| {
            let target = path.clone();
            let last = index + 1 == crumbs.len();
            div()
                .flex()
                .flex_none()
                .items_center()
                .gap(px(4.))
                .when(index > 0, |el| el.child(div().text_color(hsla(FAINT)).child("›")))
                .child(
                    div()
                        .id(("pk-crumb", index))
                        .px(px(5.))
                        .py(px(2.))
                        .rounded(px(6.))
                        .text_color(hsla(if last { TEXT } else { MUTED }))
                        .when(last, |el| el.font_weight(FontWeight::SEMIBOLD))
                        .hover(|el| el.bg(hsla(ROW_HOVER)))
                        .cursor_pointer()
                        .on_click(update(cx, move |p| p.go(Some(target.clone()))))
                        .child(label.clone()),
                )
                .into_any_element()
        }));

    let body = if current.is_none() {
        div()
            .flex_1()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(6.))
            .text_size(px(13.))
            .text_color(hsla(MUTED))
            .child(svg().path("icons/folder-open.svg").size(px(28.)).text_color(hsla(FAINT)))
            .child("Elige un lugar a la izquierda para empezar.")
            .into_any_element()
    } else if let Some(error) = &picker.error {
        div().flex_1().p(px(16.)).text_size(px(12.)).text_color(hsla(0xf07b6e)).child(format!("No se pudo abrir: {error}")).into_any_element()
    } else if visible.is_empty() {
        div()
            .flex_1()
            .p(px(16.))
            .text_size(px(12.))
            .text_color(hsla(MUTED))
            .child(if picker.filter.is_empty() { "Sin subcarpetas." } else { "Ninguna coincide con el filtro." })
            .into_any_element()
    } else {
        div()
            .id("picker-list")
            .flex_1()
            .min_h(px(0.))
            .px(px(8.))
            .flex()
            .flex_col()
            .gap(px(1.))
            .overflow_y_scroll()
            .children(visible.iter().enumerate().map(|(index, entry)| {
                let path = entry.path.clone();
                let toggled = entry.path.clone();
                let selected = picker.is_selected(&entry.path);
                div()
                    .id(("pk-row", index))
                    .h(px(32.))
                    .px(px(8.))
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(10.))
                    .rounded(px(8.))
                    .when(index == picker.cursor, |el| el.bg(hsla(ROW_ON)))
                    .hover(|el| el.bg(hsla(ROW_HOVER)))
                    .cursor_pointer()
                    .on_click(update(cx, move |p| p.go(Some(path.clone()))))
                    .child(svg().path("icons/folder.svg").size(px(14.)).flex_none().text_color(hsla(if selected { ACCENT } else { MUTED })))
                    .child(
                        div()
                            .flex_1()
                            .truncate()
                            .text_size(px(12.5))
                            .text_color(hsla(if entry.hidden { FAINT } else { TEXT }))
                            .child(entry.name.clone()),
                    )
                    .child(
                        div()
                            .id(("pk-check", index))
                            .p(px(4.))
                            .rounded(px(12.))
                            .hover(|el| el.bg(hsla(ROW_ON)))
                            .tooltip(crate::hover::tip("Marcar esta carpeta"))
                            .on_click(cx.listener(move |view: &mut SpaceView, _: &ClickEvent, _, cx| {
                                cx.stop_propagation();
                                if let Some(p) = &mut view.picker {
                                    p.toggle(&toggled);
                                    cx.notify();
                                }
                            }))
                            .child(check(selected)),
                    )
                    .into_any_element()
            }))
            .into_any_element()
    };

    let count = picker.selected.len();
    let footer = div()
        .flex()
        .items_center()
        .gap(px(8.))
        .px(px(14.))
        .py(px(12.))
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .flex()
                .flex_wrap()
                .gap(px(6.))
                .when(count == 0, |el| {
                    el.child(div().text_size(px(12.)).text_color(hsla(FAINT)).child("Marca una o varias carpetas con el círculo."))
                })
                .children(picker.selected.iter().enumerate().map(|(index, path)| {
                    let target = path.clone();
                    div()
                        .id(("pk-chosen", index))
                        .h(px(26.))
                        .pl(px(10.))
                        .pr(px(4.))
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .rounded(px(13.))
                        .bg(hsla(ROW_ON))
                        .text_size(px(12.))
                        .text_color(hsla(TEXT))
                        .tooltip(crate::hover::tip_text(path.display().to_string().into()))
                        .child(short_name(path))
                        .child(
                            div()
                                .id(("pk-unchoose", index))
                                .size(px(18.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded(px(9.))
                                .hover(|el| el.bg(hsla(0x3a3a36)))
                                .cursor_pointer()
                                .on_click(update(cx, move |p| p.toggle(&target)))
                                .child(svg().path("icons/x.svg").size(px(10.)).text_color(hsla(MUTED))),
                        )
                        .into_any_element()
                })),
        )
        .child(
            div()
                .id("pk-cancel")
                .h(px(30.))
                .px(px(14.))
                .flex()
                .flex_none()
                .items_center()
                .rounded(px(15.))
                .text_size(px(12.))
                .text_color(hsla(TEXT))
                .bg(hsla(0x2a2a28))
                .hover(|el| el.bg(hsla(0x353532)))
                .cursor_pointer()
                .on_click(cx.listener(|view: &mut SpaceView, _: &ClickEvent, _, cx| {
                    view.picker = None;
                    cx.notify();
                }))
                .child("Cancelar"),
        )
        .child(
            div()
                .id("pk-confirm")
                .h(px(30.))
                .px(px(14.))
                .flex()
                .flex_none()
                .items_center()
                .rounded(px(15.))
                .text_size(px(12.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(hsla(if count > 0 { INK } else { FAINT }))
                .bg(hsla(if count > 0 { 0xe9e9e2 } else { 0x2a2a28 }))
                .when(count > 0, |el| el.cursor_pointer().hover(|el| el.bg(hsla(0xffffff))))
                .on_click(cx.listener(|view: &mut SpaceView, _: &ClickEvent, _, cx| view.confirm_picker(cx)))
                .child(if count > 0 { format!("{action} ({count})") } else { action.to_string() }),
        );

    let main = div()
        .flex_1()
        .min_w(px(0.))
        .h_full()
        .flex()
        .flex_col()
        .child(
            div()
                .px(px(14.))
                .pt(px(14.))
                .pb(px(6.))
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(div().text_size(px(15.)).font_weight(FontWeight::SEMIBOLD).text_color(hsla(TEXT)).child(title))
                .child(
                    div()
                        .text_size(px(11.5))
                        .text_color(hsla(MUTED))
                        .child("Clic en el nombre para entrar; el círculo la marca. Escribe para filtrar, Ctrl+Enter confirma."),
                ),
        )
        .when(current.is_some(), |el| {
            el.child(
                div()
                    .px(px(14.))
                    .py(px(6.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(path_bar)
                    .when(!picker.filter.is_empty(), |el| {
                        el.child(
                            div()
                                .flex_none()
                                .h(px(24.))
                                .px(px(9.))
                                .flex()
                                .items_center()
                                .gap(px(5.))
                                .rounded(px(12.))
                                .bg(hsla(ROW_ON))
                                .text_size(px(11.5))
                                .text_color(hsla(TEXT))
                                .child(svg().path("icons/search.svg").size(px(11.)).text_color(hsla(MUTED)))
                                .child(picker.filter.clone()),
                        )
                    })
                    .child(
                        div()
                            .id("pk-here")
                            .flex_none()
                            .h(px(26.))
                            .px(px(10.))
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .rounded(px(13.))
                            .bg(hsla(if here_selected { ACCENT } else { 0x2a2a28 }))
                            .text_size(px(12.))
                            .text_color(hsla(if here_selected { INK } else { TEXT }))
                            .hover(|el| el.opacity(0.9))
                            .cursor_pointer()
                            .on_click(update(cx, |p| {
                                if let Some(dir) = p.dir.clone() {
                                    p.toggle(&dir);
                                }
                            }))
                            .child(if here_selected { "Marcada" } else { "Marcar esta" }),
                    ),
            )
        })
        .child(body)
        .child(footer);

    Some(
        div()
            .id("picker-veil")
            .absolute()
            .inset_0()
            .bg(gpui::black().opacity(0.55))
            .flex()
            .items_center()
            .justify_center()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|view: &mut SpaceView, _: &MouseDownEvent, _, cx| {
                    view.picker = None;
                    cx.notify();
                }),
            )
            .child(
                div()
                    .id("picker")
                    .w(px(w))
                    .h(px(h))
                    .flex()
                    .rounded(px(16.))
                    .bg(hsla(MODAL))
                    .shadow_lg()
                    .occlude()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(side)
                    .child(main)
                    .with_animation(
                        "picker-in",
                        Animation::new(Duration::from_millis(180)).with_easing(ease_in_out),
                        |el, t| el.opacity(t).mt(px(10.0 * (1.0 - t))),
                    ),
            ),
    )
}
