//! Panel flotante del Clipboard: buscador, filtros y lista virtualizada.
//! Medidas y textos de `ClipboardFloat.svelte` y `ClipboardHistoryList.svelte`
//! en modo isla. Los datos son de prueba.

use std::sync::Arc;

use gpui::{
    actions, div, img, prelude::*, px, rgb, svg, uniform_list, App, ClickEvent, Context, Entity,
    EventEmitter, FocusHandle, Focusable, FontWeight, Hsla, Image, ImageFormat, KeyBinding,
    MouseButton, MouseDownEvent, MouseMoveEvent, Pixels, Point, ScrollStrategy, SharedString,
    Subscription, UniformListScrollHandle, Window,
};

use crate::text_input::{self, TextInput};

pub const PANEL_W: f32 = 312.0;
pub const PANEL_H: f32 = 372.0;
const ROW_H: f32 = 44.0;
const ROW_GAP: f32 = 2.0;
/// Lo que hay que mover el cursor con el botón apretado para que sea arrastre
/// y no clic (`ClipboardHistoryList.svelte`).
const DRAG_THRESHOLD: f32 = 6.0;

actions!(clipboard_panel, [SelectPrev, SelectNext, Confirm, Dismiss]);

const KEY_CONTEXT: &str = "ClipboardPanel";

pub fn bind_keys(cx: &mut App) {
    let context = Some(KEY_CONTEXT);
    cx.bind_keys([
        KeyBinding::new("up", SelectPrev, context),
        KeyBinding::new("down", SelectNext, context),
        KeyBinding::new("enter", Confirm, context),
        KeyBinding::new("escape", Dismiss, context),
    ]);
}

#[derive(Clone)]
pub enum Content {
    Text(SharedString),
    Color(SharedString, Hsla),
    Image(Arc<Image>),
}

#[derive(Clone)]
pub struct Entry {
    pub id: usize,
    pub content: Content,
    preview: SharedString,
    when: SharedString,
    pinned: bool,
}

#[derive(Clone, Copy, PartialEq)]
enum Filter {
    All,
    Text,
    Images,
}

pub enum PanelEvent {
    Paste(Entry),
    Drag(Entry),
    Close,
}

struct IconButton {
    id: &'static str,
    icon: &'static str,
    /// Lado del botón y del ícono.
    size: (f32, f32),
    active: bool,
    idle_color: Hsla,
}

struct Colors {
    text: Hsla,
    muted: Hsla,
    faint: Hsla,
    header_icon: Hsla,
}

pub struct ClipboardPanel {
    entries: Vec<Entry>,
    visible: Vec<usize>,
    search: Entity<TextInput>,
    filter: Filter,
    favorites_only: bool,
    selected: usize,
    pub pinned: bool,
    scroll: UniformListScrollHandle,
    colors: Colors,
    /// Fila apretada y dónde, hasta que se suelta o empieza el arrastre.
    press: Option<(usize, Point<Pixels>)>,
    /// El último gesto terminó en arrastre: el clic que le sigue no pega.
    dragged: bool,
    _search_changed: Subscription,
}

impl EventEmitter<PanelEvent> for ClipboardPanel {}

impl Focusable for ClipboardPanel {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.search.focus_handle(cx)
    }
}

impl ClipboardPanel {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let colors = Colors {
            text: rgb(0xf0f0ea).into(),
            muted: rgb(0x9a9a90).into(),
            faint: rgb(0x6e6e66).into(),
            header_icon: rgb(0x8f8f86).into(),
        };
        let search =
            cx.new(|cx| TextInput::new("Buscar…", colors.text, colors.muted, colors.text, cx));
        let search_changed = cx.subscribe(&search, |panel, _, _: &text_input::Changed, cx| {
            panel.refilter(cx);
        });
        let mut panel = Self {
            entries: mock_entries(),
            visible: Vec::new(),
            search,
            filter: Filter::All,
            favorites_only: false,
            selected: 0,
            pinned: false,
            scroll: UniformListScrollHandle::new(),
            colors,
            press: None,
            dragged: false,
            _search_changed: search_changed,
        };
        panel.refilter(cx);
        panel
    }

    /// Al abrir: buscador vacío y la lista arriba, como en Atic.
    pub fn reset(&mut self, cx: &mut Context<Self>) {
        self.search.update(cx, |search, cx| search.clear(cx));
        self.selected = 0;
        self.scroll.scroll_to_item(0, ScrollStrategy::Top);
    }

    fn refilter(&mut self, cx: &mut Context<Self>) {
        let query = fold(self.search.read(cx).text());
        let tokens: Vec<&str> = query.split_whitespace().collect();
        self.visible = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| {
                let kind_ok = match self.filter {
                    Filter::All => true,
                    Filter::Text => !matches!(entry.content, Content::Image(_)),
                    Filter::Images => matches!(entry.content, Content::Image(_)),
                };
                let haystack = fold(&entry.preview);
                let query_ok = haystack.contains(query.trim())
                    || tokens.iter().all(|token| haystack.contains(token));
                kind_ok && (!self.favorites_only || entry.pinned) && query_ok
            })
            .map(|(index, _)| index)
            .collect();
        self.selected = 0;
        self.scroll.scroll_to_item(0, ScrollStrategy::Top);
        cx.notify();
    }

    fn select_prev(&mut self, _: &SelectPrev, _: &mut Window, cx: &mut Context<Self>) {
        self.selected = self.selected.saturating_sub(1);
        self.scroll
            .scroll_to_item(self.selected, ScrollStrategy::Top);
        cx.notify();
    }

    fn select_next(&mut self, _: &SelectNext, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected + 1 < self.visible.len() {
            self.selected += 1;
        }
        self.scroll
            .scroll_to_item(self.selected, ScrollStrategy::Top);
        cx.notify();
    }

    fn confirm(&mut self, _: &Confirm, _: &mut Window, cx: &mut Context<Self>) {
        self.paste(self.selected, cx);
    }

    fn dismiss(&mut self, _: &Dismiss, _: &mut Window, cx: &mut Context<Self>) {
        if !self.pinned {
            cx.emit(PanelEvent::Close);
        }
    }

    fn paste(&mut self, visible_index: usize, cx: &mut Context<Self>) {
        if let Some(&index) = self.visible.get(visible_index) {
            cx.emit(PanelEvent::Paste(self.entries[index].clone()));
        }
    }

    fn maybe_start_drag(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        let Some((index, origin)) = self.press else {
            return;
        };
        if event.pressed_button != Some(MouseButton::Left) {
            self.press = None;
            return;
        }
        let delta = event.position - origin;
        let distance = f32::from(delta.x).hypot(f32::from(delta.y));
        if distance < DRAG_THRESHOLD {
            return;
        }
        self.press = None;
        self.dragged = true;
        if let Some(entry) = self.entries.get(index) {
            cx.emit(PanelEvent::Drag(entry.clone()));
        }
    }

    fn toggle_favorite(&mut self, index: usize, cx: &mut Context<Self>) {
        self.entries[index].pinned = !self.entries[index].pinned;
        if self.favorites_only {
            self.refilter(cx);
        }
        cx.notify();
    }

    fn remove(&mut self, index: usize, cx: &mut Context<Self>) {
        self.entries.remove(index);
        self.refilter(cx);
    }

    fn set_filter(&mut self, filter: Filter, cx: &mut Context<Self>) {
        self.filter = filter;
        self.refilter(cx);
    }

    fn icon_button(
        &self,
        button: IconButton,
        on_click: impl Fn(&mut Self, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let IconButton {
            id,
            icon,
            size: (box_size, icon_size),
            active,
            idle_color,
        } = button;
        let text = self.colors.text;
        div()
            .id(id)
            .size(px(box_size))
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .rounded(px(box_size / 2.0))
            .when(active, |el| el.bg(text.opacity(0.14)))
            .hover(|el| el.bg(text.opacity(0.08)))
            .on_click(cx.listener(move |panel, _: &ClickEvent, _, cx| on_click(panel, cx)))
            .child(svg().path(icon).size(px(icon_size)).text_color(if active {
                text
            } else {
                idle_color
            }))
    }

    fn render_row(&self, visible_index: usize, cx: &mut Context<Self>) -> impl IntoElement {
        let index = self.visible[visible_index];
        let entry = &self.entries[index];
        let colors = &self.colors;
        let selected = visible_index == self.selected;
        let group = SharedString::from(format!("clip-row-{}", entry.id));

        let (thumb, kind) = match &entry.content {
            Content::Text(_) => (
                div()
                    .size(px(22.))
                    .rounded(px(6.))
                    .bg(colors.text.opacity(0.07))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        svg()
                            .path("icons/type.svg")
                            .size(px(13.))
                            .text_color(colors.muted),
                    )
                    .into_any_element(),
                "texto",
            ),
            Content::Color(_, color) => (
                div()
                    .size(px(22.))
                    .rounded(px(6.))
                    .bg(*color)
                    .border_1()
                    .border_color(colors.text.opacity(0.12))
                    .into_any_element(),
                "color",
            ),
            Content::Image(image) => (
                img(image.clone())
                    .w(px(44.))
                    .h(px(34.))
                    .rounded(px(6.))
                    .object_fit(gpui::ObjectFit::Cover)
                    .into_any_element(),
                "imagen",
            ),
        };

        let pinned = entry.pinned;
        let text = colors.text;
        let faint = colors.faint;
        div().h(px(ROW_H + ROW_GAP)).pb(px(ROW_GAP)).child(
            div()
                .id(("clip-row", entry.id))
                .group(group.clone())
                .h(px(ROW_H))
                .flex()
                .items_center()
                .gap(px(6.4))
                .px(px(6.4))
                .py(px(3.2))
                .rounded(px(12.))
                .when(selected, |el| el.bg(text.opacity(0.08)))
                .hover(|el| el.bg(text.opacity(0.08)))
                .cursor_pointer()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |panel, event: &MouseDownEvent, _, _| {
                        panel.press = Some((index, event.position));
                        panel.dragged = false;
                    }),
                )
                .on_mouse_move(cx.listener(|panel, event: &MouseMoveEvent, _, cx| {
                    panel.maybe_start_drag(event, cx)
                }))
                .on_click(cx.listener(move |panel, _: &ClickEvent, _, cx| {
                    panel.press = None;
                    if !panel.dragged {
                        panel.paste(visible_index, cx)
                    }
                }))
                .child(
                    div()
                        .w(px(44.))
                        .flex()
                        .flex_none()
                        .justify_center()
                        .child(thumb),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_w_0()
                        .gap(px(1.))
                        .child(
                            div()
                                .text_size(px(11.))
                                .font_weight(FontWeight::MEDIUM)
                                .truncate()
                                .child(entry.preview.clone()),
                        )
                        .child(
                            div()
                                .text_size(px(9.))
                                .text_color(colors.muted)
                                .truncate()
                                .child(format!("{kind} · {}", entry.when)),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_none()
                        .child(
                            div()
                                .id(("clip-star", entry.id))
                                .size(px(20.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded(px(10.))
                                .when(!pinned, |el| {
                                    el.invisible().group_hover(group.clone(), |el| el.visible())
                                })
                                .hover(|el| el.bg(text.opacity(0.08)))
                                .on_click(cx.listener(move |panel, _: &ClickEvent, _, cx| {
                                    cx.stop_propagation();
                                    panel.toggle_favorite(index, cx)
                                }))
                                .child(
                                    svg()
                                        .path("icons/star.svg")
                                        .size(px(12.))
                                        .text_color(if pinned { text } else { faint }),
                                ),
                        )
                        .child(
                            div()
                                .id(("clip-remove", entry.id))
                                .size(px(20.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded(px(10.))
                                .invisible()
                                .group_hover(group, |el| el.visible())
                                .hover(|el| el.bg(text.opacity(0.08)))
                                .on_click(cx.listener(move |panel, _: &ClickEvent, _, cx| {
                                    cx.stop_propagation();
                                    panel.remove(index, cx)
                                }))
                                .child(svg().path("icons/x.svg").size(px(12.)).text_color(faint)),
                        ),
                ),
        )
    }

    fn empty_message(&self) -> (&'static str, Option<&'static str>) {
        if self.entries.is_empty() {
            (
                "El historial está vacío",
                Some("Copia algo y aparece aquí."),
            )
        } else if self.favorites_only && !self.entries.iter().any(|entry| entry.pinned) {
            ("No hay favoritos. Márcalos con la estrella.", None)
        } else {
            ("Nada coincide", None)
        }
    }
}

impl Render for ClipboardPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = &self.colors;
        let (text, muted, header_icon) = (colors.text, colors.muted, colors.header_icon);

        let header = div()
            .h(px(24.))
            .flex_none()
            .relative()
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .w(px(32.))
                    .h(px(3.))
                    .rounded(px(2.))
                    .bg(text.opacity(0.24)),
            )
            .child(
                div()
                    .absolute()
                    .right(px(0.))
                    .top(px(-2.))
                    .flex()
                    .child(self.icon_button(
                        IconButton {
                            id: "clip-pin",
                            icon: "icons/pin.svg",
                            size: (28., 13.),
                            active: self.pinned,
                            idle_color: header_icon,
                        },
                        |panel, cx| {
                            panel.pinned = !panel.pinned;
                            cx.notify();
                        },
                        cx,
                    ))
                    .child(self.icon_button(
                        IconButton {
                            id: "clip-close",
                            icon: "icons/x.svg",
                            size: (28., 14.),
                            active: false,
                            idle_color: header_icon,
                        },
                        |_, cx| cx.emit(PanelEvent::Close),
                        cx,
                    )),
            );

        let toolbar = div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(4.))
            .mt(px(4.))
            .mb(px(6.))
            .child(
                div()
                    .h(px(25.6))
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap(px(5.))
                    .pl(px(6.4))
                    .pr(px(10.))
                    .rounded_full()
                    .bg(text.opacity(0.07))
                    .child(
                        svg()
                            .path("icons/search.svg")
                            .size(px(12.))
                            .flex_none()
                            .text_color(muted),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(px(10.))
                            .line_height(px(14.))
                            .font_weight(FontWeight::MEDIUM)
                            .child(self.search.clone()),
                    ),
            )
            .child(self.icon_button(
                IconButton {
                    id: "clip-all",
                    icon: "icons/layers.svg",
                    size: (25.6, 12.),
                    active: self.filter == Filter::All,
                    idle_color: muted,
                },
                |panel, cx| panel.set_filter(Filter::All, cx),
                cx,
            ))
            .child(self.icon_button(
                IconButton {
                    id: "clip-text",
                    icon: "icons/type.svg",
                    size: (25.6, 12.),
                    active: self.filter == Filter::Text,
                    idle_color: muted,
                },
                |panel, cx| panel.set_filter(Filter::Text, cx),
                cx,
            ))
            .child(self.icon_button(
                IconButton {
                    id: "clip-images",
                    icon: "icons/image.svg",
                    size: (25.6, 12.),
                    active: self.filter == Filter::Images,
                    idle_color: muted,
                },
                |panel, cx| panel.set_filter(Filter::Images, cx),
                cx,
            ))
            .child(self.icon_button(
                IconButton {
                    id: "clip-favorites",
                    icon: "icons/star.svg",
                    size: (25.6, 12.),
                    active: self.favorites_only,
                    idle_color: muted,
                },
                |panel, cx| {
                    panel.favorites_only = !panel.favorites_only;
                    panel.refilter(cx);
                },
                cx,
            ));

        let body = if self.visible.is_empty() {
            let (title, hint) = self.empty_message();
            div()
                .flex_1()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(4.))
                .text_size(px(11.))
                .text_color(muted)
                .child(title)
                .when_some(hint, |el, hint| {
                    el.child(
                        div()
                            .text_size(px(10.))
                            .text_color(colors.faint)
                            .child(hint),
                    )
                })
                .into_any_element()
        } else {
            uniform_list(
                "clipboard-items",
                self.visible.len(),
                cx.processor(|panel, range: std::ops::Range<usize>, _, cx| {
                    range
                        .map(|index| panel.render_row(index, cx))
                        .collect::<Vec<_>>()
                }),
            )
            .track_scroll(self.scroll.clone())
            .flex_1()
            .pb(px(8.))
            .into_any_element()
        };

        div()
            .key_context(KEY_CONTEXT)
            .on_action(cx.listener(Self::select_prev))
            .on_action(cx.listener(Self::select_next))
            .on_action(cx.listener(Self::confirm))
            .on_action(cx.listener(Self::dismiss))
            .size_full()
            .flex()
            .flex_col()
            .pt(px(7.2))
            .px(px(8.))
            .pb(px(8.8))
            .font_family("Segoe UI")
            .text_color(text)
            .child(header)
            .child(toolbar)
            .child(body)
    }
}

/// Minúsculas y sin tildes, como `clipboardSearch.ts`.
fn fold(text: &str) -> String {
    text.to_lowercase()
        .chars()
        .map(|ch| match ch {
            'á' | 'à' | 'ä' | 'â' => 'a',
            'é' | 'è' | 'ë' | 'ê' => 'e',
            'í' | 'ì' | 'ï' | 'î' => 'i',
            'ó' | 'ò' | 'ö' | 'ô' => 'o',
            'ú' | 'ù' | 'ü' | 'û' => 'u',
            'ñ' => 'n',
            other => other,
        })
        .collect()
}

fn mock_image(bytes: &'static [u8]) -> Arc<Image> {
    Arc::new(Image::from_bytes(ImageFormat::Png, bytes.to_vec()))
}

/// 100 entradas (el máximo de Atic) para probar el scroll virtualizado.
fn mock_entries() -> Vec<Entry> {
    let texts = [
        "Reunión con el equipo de plataforma el jueves a las 10:30",
        "https://github.com/zed-industries/zed/tree/main/crates/gpui",
        "SELECT id, nombre, db_name FROM mantenedor.cliente WHERE activo = 1;",
        "pnpm --dir apps/desktop exec vitest run src/lib/clipboardSearch.test.ts",
        "Gracias por el envío, lo reviso mañana temprano y te confirmo.",
        "cargo test --locked -p atic-core historial",
        "Dirección: Av. Providencia 1234, oficina 56, Santiago",
        "fn main() { println!(\"hola desde la pill\"); }",
        "El informe trimestral quedó en la carpeta compartida de Operaciones",
        "calcantara@ejemplo.cl",
        "https://claude.ai/code",
        "Pendiente: revisar la animación de la rueda en monitores 150 %",
        "TODO(pill): mover la isla cuando hay notch",
        "Contraseña del wifi de invitados: pídela en recepción",
        "{ \"tema\": \"atic\", \"modo\": \"oscuro\", \"rueda\": 9 }",
        "Número de seguimiento: 7AB3-55Q2-910Z",
    ];
    let colors: [(&str, u32); 5] = [
        ("#e85a52", 0xe85a52),
        ("#6faf88", 0x6faf88),
        ("rgb(212, 168, 75)", 0xd4a84b),
        ("#8fa9b8", 0x8fa9b8),
        ("#1a1a18", 0x1a1a18),
    ];
    let images = [
        (
            "Captura · gráfico de ventas",
            include_bytes!("../assets/mock/grafico.png").as_slice(),
        ),
        (
            "Captura · ventana de la app",
            include_bytes!("../assets/mock/ventana.png").as_slice(),
        ),
        (
            "atardecer.png",
            include_bytes!("../assets/mock/atardecer.png").as_slice(),
        ),
        (
            "Ícono de Atic",
            include_bytes!("../assets/mock/icono.png").as_slice(),
        ),
    ];
    let images: Vec<(&str, Arc<Image>)> = images
        .into_iter()
        .map(|(name, bytes)| (name, mock_image(bytes)))
        .collect();

    let days = ["lun", "mar", "mié", "jue", "vie", "sáb", "dom"];
    let months = ["ago", "sep"];
    (0..100)
        .map(|id| {
            let when: SharedString = match id {
                0..=11 => format!("{:02}:{:02}", 16 - id / 4, 59 - (id * 7) % 60),
                12..=24 => format!("ayer · {:02}:{:02}", 20 - (id - 12) / 2, (id * 13) % 60),
                25..=59 => format!("{} · {:02}:{:02}", days[id % 7], 9 + id % 9, (id * 11) % 60),
                _ => format!(
                    "{} {} {:02}:{:02}",
                    1 + id % 28,
                    months[id % 2],
                    8 + id % 10,
                    (id * 17) % 60
                ),
            }
            .into();
            let (content, preview): (Content, SharedString) = match id % 9 {
                4 => {
                    let (label, hex) = colors[id % colors.len()];
                    (Content::Color(label.into(), rgb(hex).into()), label.into())
                }
                7 => {
                    let (name, image) = &images[id % images.len()];
                    (Content::Image(image.clone()), (*name).into())
                }
                _ => {
                    let text: SharedString = texts[id % texts.len()].into();
                    (Content::Text(text.clone()), text)
                }
            };
            Entry {
                id,
                content,
                preview,
                when,
                pinned: id % 13 == 2,
            }
        })
        .collect()
}
