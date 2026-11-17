//! Cómo se ve el uso de los agentes: el modo simple (anillos), el detalle
//! (tarjetas) y el personalizador, que se abre manteniendo presionado un
//! agente (o con el lápiz) y se usa como el editor de la pill de Atic
//! (`PillCustomize.svelte`): dos filas, «A la vista» y «Fuera de la vista»,
//! con fichas que se arrastran para ordenar o pasar de una a otra. Un clic
//! en una ficha la cambia de fila. Cada cambio se guarda al tiro.

use std::time::{Duration, Instant};

use gpui::{
    canvas, div, prelude::*, px, svg, AnyElement, Bounds, Context, FontWeight, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, Pixels, SharedString,
};

use super::*;
use crate::hover::{m3_layer, HoverExt};
use crate::theme::SmallText;
use gpui_m3::theme::type_scale::{self, TypeScaleExt};
use gpui_m3::{corner, Scheme, Spring};

/// Mantener presionado un agente abre el personalizador.
const HOLD: Duration = Duration::from_millis(450);
/// Lo que hay que mover una ficha para que sea arrastre y no clic.
const DRAG_SLOP: f32 = 4.0;

/// El personalizador: fichas de ancho fijo que se acomodan en filas.
const CHIP_W: f32 = 118.0;
const CHIP_H: f32 = 34.0;
const CHIP_GAP: f32 = 6.0;
const LANE_LABEL: f32 = 22.0;
const LANE_GAP: f32 = 10.0;
const NOTE_H: f32 = 20.0;

/// Cuántas fichas caben por línea en un ancho.
fn per_line(width: f32) -> usize {
    (((width - SIDE * 2.0 + CHIP_GAP) / (CHIP_W + CHIP_GAP)).floor() as usize).max(1)
}

/// El ancho de cada ficha para que la línea se llene entera (sin espacio
/// muerto a la derecha).
fn chip_width(width: f32) -> f32 {
    let per = per_line(width) as f32;
    ((width - SIDE * 2.0 - (per - 1.0) * CHIP_GAP) / per).max(CHIP_W)
}

/// En el bloque angosto de un costado (`SIDE_W`) los anillos van de a tres
/// por fila y las tarjetas del detalle, una por fila.
const SIDE_RINGS: usize = 3;
const RING_ROW_GAP: f32 = 4.0;

/// Cuántos anillos o tarjetas van por fila.
fn per_row(narrow: bool, simple: bool) -> usize {
    match (narrow, simple) {
        (true, true) => SIDE_RINGS,
        (true, false) => 1,
        (false, true) => usize::MAX,
        (false, false) => 2,
    }
}

fn lane_height(count: usize, per: usize) -> f32 {
    let lines = count.max(1).div_ceil(per) as f32;
    LANE_LABEL + lines * CHIP_H + (lines - 1.0) * CHIP_GAP
}

impl Pill {
    /// El alto que gana el tab: depende del modo, de cuántos agentes se ven y
    /// de si hay un detalle o el personalizador abierto. No mira `shape()`
    /// (que lo usa): el ancho es el de la tira abierta, o el del bloque
    /// angosto en un costado.
    pub(crate) fn usage_height(&self) -> f32 {
        self.usage.animated_height(self.usage_target_height())
    }

    /// El alto al que va el vistazo (sin animar).
    fn usage_target_height(&self) -> f32 {
        self.usage_measure(false)
    }

    /// El alto de su página en el carrusel de vistazos: con el lugar del
    /// detalle ya reservado, así pasar por un agente no agranda el notch (y
    /// la página siguiente no se achica bajo el cursor).
    pub(crate) fn usage_page_height(&self) -> f32 {
        self.usage_measure(true)
    }

    fn usage_measure(&self, reserve_detail: bool) -> f32 {
        let snap = self.usage.snapshot();
        let prefs = self.usage.prefs();
        let view = self.usage.view();
        let narrow = self.side_drawers();
        if view.editing {
            let width = if narrow { crate::SIDE_W } else { PANEL_W };
            let per = per_line(width);
            let (shown, out) = arranged(&snap, &prefs);
            let note = if view.refused { NOTE_H } else { 0.0 };
            return HEADER_H + lane_height(shown.len(), per) + LANE_GAP + lane_height(out.len(), per) + note + BOTTOM;
        }
        let shown = visible(&snap, &prefs);
        if prefs.simple {
            // El detalle mide lo que su tarjeta; para reservar, la más alta.
            let focused = view.focus.and_then(|id| shown.iter().find(|r| r.family.id == id)).map(card_height);
            let tallest = shown.iter().map(card_height).fold(None, |a: Option<f32>, h| Some(a.map_or(h, |a| a.max(h))));
            let card = if reserve_detail { tallest } else { focused };
            let detail = card.map_or(0.0, |h| GAP + h);
            let rows = shown.len().max(1).div_ceil(per_row(narrow, true)) as f32;
            return HEADER_H + rows * SIMPLE_H + (rows - 1.0) * RING_ROW_GAP + detail + BOTTOM;
        }
        let lines: Vec<f32> = shown.chunks(per_row(narrow, false)).map(line_height).collect();
        let cards = if lines.is_empty() { card_height_for(0) } else { lines.iter().sum::<f32>() };
        HEADER_H + cards + (lines.len().max(1) as f32 - 1.0) * GAP + BOTTOM
    }

    /// El cursor sobre el vistazo de uso (la bandeja lo cuenta como suyo).
    pub(crate) fn usage_over(&self, p: (f32, f32), now: Instant) -> bool {
        self.usage_rect(now)
            .or_else(|| self.hosted_rect(crate::AGENTES_TOOL, now))
            .is_some_and(|r| r.contains(p, 4.0))
    }

    pub(super) fn usage_rect(&self, now: Instant) -> Option<Rect> {
        if self.usage_peek.value(now) <= 0.01 {
            return None;
        }
        self.drawer_rect(Drawer::Usage, now)
    }

    pub(crate) fn usage_element(&self, now: Instant, cx: &mut Context<Self>) -> Option<AnyElement> {
        let amount = self.usage_peek.value(now);
        let area = self.usage_rect(now)?;
        Some(self.usage_view(area, (0.0, 0.0), amount, cx))
    }

    /// El vistazo de uso en `area` (en la ventana), dibujado dentro de algo
    /// cuya esquina está en `origin`: el de siempre en la ventana, o una página
    /// del carrusel de vistazos (`peeks.rs`).
    pub(crate) fn usage_view(&self, area: Rect, origin: (f32, f32), amount: f32, cx: &mut Context<Self>) -> AnyElement {
        let narrow = self.side_drawers();
        // En Expressive, los roles del esquema de la pill (claro u oscuro).
        let m3 = crate::pill_look::content_scheme();
        let palette = match m3 {
            Some(s) => crate::Palette { skin: s.surface_container, text: s.on_surface, muted: s.on_surface_variant },
            None => crate::Palette::dark(),
        };
        let snap = self.usage.snapshot();
        let prefs = self.usage.prefs();
        let view = self.usage.view();
        // Las fichas y las filas vuelven a anotar dónde quedaron al pintarse.
        self.usage.update_view(|v| {
            v.chips.clear();
            v.lanes = [None, None];
        });

        // Al abrir, cada agente entra un poco después del anterior y su
        // anillo se llena desde cero; el detalle y el cambio de modo, con un
        // fundido corto.
        // Si los datos llegan después de abrir (la primera lectura tarda),
        // la entrada parte cuando llegan: los anillos se llenan a la vista.
        let opened = Usage::since(self.usage.intro_start(&view));
        let enter = |i: usize| crate::anim::ease_smooth_out(segment(opened, 0.05 + i as f32 * 0.06, 0.55));
        let switched = crate::anim::ease_smooth_out(segment(Usage::since(view.mode_at), 0.0, 0.25));
        let focused = crate::anim::ease_smooth_out(segment(Usage::since(view.focus_at), 0.0, 0.28));
        let body: AnyElement = if view.editing {
            editor(&snap, &prefs, &view, &self.usage, chip_width(area.w), &palette, m3, cx)
        } else if let (None, Some(s)) = (snap.at, m3) {
            // Leyendo por primera vez, en Expressive: las tarjetas con su pista
            // vacía, quietas (nada late en bucle).
            div()
                .px(px(SIDE))
                .h(px(SIMPLE_H))
                .flex()
                .items_center()
                .gap(px(ITEM_GAP_M3))
                .children((0..4).map(|_| {
                    div()
                        .flex_1()
                        .h_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(corner::LI)
                        .bg(s.surface_container_high)
                        .child(div().size(px(RING)).rounded_full().border_4().border_color(s.secondary_container))
                }))
                .into_any_element()
        } else if snap.at.is_none() {
            // Leyendo por primera vez: anillos vacíos que laten.
            let pulse = 0.35 + 0.25 * (Usage::since(view.opened_at) * 4.0).sin().abs();
            div()
                .px(px(SIDE))
                .h(px(SIMPLE_H))
                .flex()
                .items_center()
                .gap(px(4.))
                .children((0..4).map(|_| {
                    div().flex_1().flex().justify_center().child(
                        div()
                            .size(px(RING - 6.))
                            .rounded(px(RING))
                            .border_3()
                            .border_color(palette.text.opacity(0.12 * pulse * 2.0)),
                    )
                }))
                .into_any_element()
        } else if prefs.simple {
            let shown = visible(&snap, &prefs);
            let focus = view.focus.and_then(|id| shown.iter().find(|r| r.family.id == id));
            div()
                .px(px(SIDE))
                .flex()
                .flex_col()
                .gap(px(GAP))
                .child(
                    div().flex().flex_col().gap(px(RING_ROW_GAP)).children(
                        shown.chunks(per_row(narrow, true)).enumerate().map(|(line, chunk)| {
                            let per = per_row(narrow, true);
                            div()
                                .h(px(SIMPLE_H))
                                .flex()
                                .gap(px(if m3.is_some() { ITEM_GAP_M3 } else { 4. }))
                                .children(chunk.iter().enumerate().map(|(j, row)| {
                                    let i = line.saturating_mul(per) + j;
                                    let focused = view.focus == Some(row.family.id);
                                    match m3 {
                                        Some(s) => simple_item_m3(row, focused, enter(i), s, cx),
                                        None => simple_item(row, focused, enter(i), &palette, cx),
                                    }
                                }))
                                // La última fila incompleta no estira sus anillos.
                                .when(narrow && chunk.len() < per, |el| {
                                    el.children((chunk.len()..per).map(|_| div().flex_1()))
                                })
                        }),
                    ),
                )
                .children(focus.map(|row| {
                    // El detalle baja con un fundido; sus barras crecen.
                    div()
                        .relative()
                        .top(px(-6.0 * (1.0 - focused)))
                        .opacity(focused)
                        .child(card(row, card_height(row), focused.max(enter(0)), &palette, m3, cx))
                }))
                .into_any_element()
        } else {
            // Dos columnas que se reparten todo el ancho del notch.
            let shown = visible(&snap, &prefs);
            let per = per_row(narrow, false);
            let lines: Vec<AnyElement> = shown
                .chunks(per)
                .enumerate()
                .map(|(line, pair)| {
                    let height = line_height(pair);
                    div()
                        .flex()
                        .gap(px(GAP))
                        .children(pair.iter().enumerate().map(|(j, row)| {
                            let p = enter(line * per + j);
                            div()
                                .relative()
                                .top(px(8.0 * (1.0 - p)))
                                .opacity(p)
                                .flex_1()
                                .min_w_0()
                                .child(card(row, height, p, &palette, m3, cx))
                        }))
                        .when(pair.len() < per, |el| el.child(div().flex_1()))
                        .into_any_element()
                })
                .collect();
            div().px(px(SIDE)).flex().flex_col().gap(px(GAP)).children(lines).into_any_element()
        };

        // La ficha que se arrastra, bajo el cursor.
        let ghost = view.drag.filter(|d| d.moved).and_then(|drag| {
            let row = rows(&snap).into_iter().find(|r| r.family.id == drag.id)?;
            Some(
                div()
                    .absolute()
                    .left(px(drag.pos.0 - area.x - chip_width(area.w) / 2.0))
                    .top(px(drag.pos.1 - area.y - CHIP_H / 2.0))
                    .child(chip_face(&row, true, chip_width(area.w), &palette, m3).opacity(0.92)),
            )
        });

        div()
                .id("usage-peek")
                .absolute()
                .left(px(area.x - origin.0))
                .top(px(area.y - origin.1))
                .w(px(area.w))
                .h(px(area.h))
                .overflow_hidden()
                // Sin fondo propio: el tab entero se vuelve casi opaco
                // (el tinte al leer de Apariencia), como con la letra.
                .opacity(segment(amount, 0.55, 0.45))
                .font_family(crate::fonts::UI)
                .flex()
                .flex_col()
                .on_mouse_move(cx.listener(|pill, event: &MouseMoveEvent, _, cx| {
                    if pill.usage.drag_to((f32::from(event.position.x), f32::from(event.position.y))) {
                        cx.notify();
                    }
                }))
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|pill, _: &MouseUpEvent, _, cx| {
                        pill.usage.release();
                        cx.notify();
                    }),
                )
                .on_mouse_up_out(
                    MouseButton::Left,
                    cx.listener(|pill, _: &MouseUpEvent, _, cx| {
                        pill.usage.release();
                        cx.notify();
                    }),
                )
                .child(header(&snap, self.usage.loading(), &prefs, &view, narrow, &palette, m3, cx))
                .child(div().opacity(switched).child(body))
                .children(ghost)
                .into_any_element()
    }
}

impl Usage {
    /// Un agente presionado: si se mantiene `HOLD`, abre el personalizador.
    fn press(&self, id: &'static str, cx: &mut Context<Pill>) {
        let mut token = 0;
        self.update_view(|v| {
            v.press_seq += 1;
            token = v.press_seq;
            v.press = Some((token, id));
        });
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(HOLD).await;
            let _ = this.update(cx, |pill, cx| {
                if pill.usage.view().press.is_some_and(|(t, _)| t == token) {
                    pill.usage.set_editing(true);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Una ficha del personalizador empieza a arrastrarse.
    fn grab(&self, id: &'static str, at: (f32, f32)) {
        self.update_view(|v| {
            v.drag = Some(Drag { id, start: at, pos: at, moved: false, target: None });
            v.refused = false;
        });
    }

    /// El cursor se mueve con una ficha agarrada: dónde caería. Devuelve si
    /// hay que repintar.
    fn drag_to(&self, at: (f32, f32)) -> bool {
        let mut changed = false;
        self.update_view(|v| {
            let Some(mut drag) = v.drag else { return };
            drag.pos = at;
            drag.moved |= (at.0 - drag.start.0).hypot(at.1 - drag.start.1) > DRAG_SLOP;
            let point = gpui::point(px(at.0), px(at.1));
            drag.target = v.lanes.iter().position(|b| b.is_some_and(|b| b.contains(&point))).map(|lane| {
                let hidden = lane == 1;
                let index = v
                    .chips
                    .iter()
                    .filter(|(id, h, _)| *h == hidden && *id != drag.id)
                    .filter(|(_, _, b)| {
                        let c = b.center();
                        let (cx, cy) = (f32::from(c.x), f32::from(c.y));
                        cy < at.1 - CHIP_H / 2.0 || ((cy - at.1).abs() <= CHIP_H / 2.0 && cx < at.0)
                    })
                    .count();
                (hidden, index)
            });
            v.drag = Some(drag);
            changed = true;
        });
        changed
    }

    /// Se soltó el botón: termina un arrastre (o un clic en una ficha, que
    /// la cambia de fila) y una presión corta (que en el modo simple abre o
    /// cierra el detalle).
    fn release(&self) {
        let view = self.view();
        if let Some(drag) = view.drag {
            let hidden_now = self.prefs().hidden.iter().any(|h| h == drag.id);
            match (drag.moved, drag.target) {
                (true, Some((hidden, index))) => self.place(drag.id, hidden, index),
                (false, _) => self.place(drag.id, !hidden_now, usize::MAX),
                _ => {}
            }
        }
        if let Some((_, id)) = view.press {
            if !view.editing && self.prefs().simple {
                self.set_focus(if view.focus == Some(id) { None } else { Some(id) });
            }
        }
        self.update_view(|v| {
            v.drag = None;
            v.press = None;
        });
    }
}

/// El porcentaje de un agente: en Expressive, LABEL_LARGE enfatizado con
/// cifras tabulares (cuenta hacia arriba sin bailar). Con un alto de línea de
/// 18 para que el anillo, la cifra y el nombre quepan en `SIMPLE_H`.
fn percent_text(el: gpui::Div) -> gpui::Div {
    match crate::theme::pill_type(type_scale::LABEL_LARGE) {
        Some(role) => el.font(crate::fonts::tabular(role.emphasized)).text_size(role.size).line_height(px(18.)),
        None => el,
    }
}

/// Cifras tabulares con el peso del rol: los valores de las barras.
fn tabular_text(el: gpui::Div, role: type_scale::TypeStyle) -> gpui::Div {
    el.font(crate::fonts::tabular(role.weight)).text_size(role.size).line_height(role.line_height)
}

/// Entre las tarjetas de los agentes, en Expressive.
const ITEM_GAP_M3: f32 = 6.0;
/// El alto de los botones del encabezado en Expressive (cabe en `HEADER_H`).
const HEADER_BTN_M3: f32 = 28.0;
/// El botón de ícono del encabezado (el lápiz) en Expressive.
const ICON_BTN_M3: f32 = 32.0;

/// El color del indicador según el uso: el acento mientras sobra, `tertiary`
/// al acercarse al tope y `error` casi lleno (los umbrales de `tone`).
fn m3_tone(percent: f64, s: &Scheme) -> gpui::Hsla {
    if percent < 60.0 {
        s.primary
    } else if percent < 85.0 {
        s.tertiary
    } else {
        s.error
    }
}

/// Un clic de M3 sin capa propia: la capa de estado de `color` sobre nada.
fn clear_layer(color: gpui::Hsla, h: crate::hover::Hover) -> gpui::Hsla {
    m3_layer(color.opacity(0.0), color, h)
}

/// Un botón chico del encabezado.
fn pill_button(id: &'static str, label: &str) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .h(px(22.))
        .px(px(9.))
        .flex()
        .items_center()
        .rounded(px(11.))
        .pill_text(11., type_scale::LABEL_MEDIUM)
        .cursor_pointer()
        .child(SharedString::from(label.to_string()))
}

/// El realce de `pill_button`, con transición. Va después de `on_press`.
fn pill_fx(el: gpui::Stateful<gpui::Div>, id: &'static str, on: bool, palette: &crate::Palette) -> AnyElement {
    let (text, muted) = (palette.text, palette.muted);
    el.fx(SharedString::from(format!("{id}-fx")), move |el, h| {
        let (rest, over) = if on { (0.16, 0.18) } else { (0.0, 0.12) };
        el.bg(h.mix(text.opacity(rest), text.opacity(over)))
            .text_color(if on { text } else { h.mix(muted, text) })
    })
    .into_any_element()
}

/// Un clic que no llega a la pill de abajo (que lo tomaría como arrastrar
/// el notch).
fn on_press(
    el: gpui::Stateful<gpui::Div>,
    cx: &mut Context<Pill>,
    action: impl Fn(&mut Pill, &mut Context<Pill>) + 'static,
) -> gpui::Stateful<gpui::Div> {
    el.on_mouse_down(
        MouseButton::Left,
        cx.listener(move |pill, _: &MouseDownEvent, _, cx| {
            action(pill, cx);
            cx.stop_propagation();
            cx.notify();
        }),
    )
}

/// Título, edad del dato y los controles. En el personalizador: Restablecer
/// y Listo.
fn header(
    snap: &Snapshot,
    loading: bool,
    prefs: &Prefs,
    view: &View,
    narrow: bool,
    palette: &crate::Palette,
    m3: Option<Scheme>,
    cx: &mut Context<Pill>,
) -> AnyElement {
    let status = match snap.at {
        None => crate::i18n::t("pill.activity.reading").to_string(),
        Some(_) if loading => crate::i18n::t("page.agents.usageModal.refreshing").to_string(),
        Some(at) => match at.elapsed().as_secs() {
            0..=4 => crate::i18n::t("pill.usage.justNow").to_string(),
            s if s < 60 => crate::i18n::tf("pill.usage.agoSeconds", &[("s", &s)]),
            s => crate::i18n::tf("pill.usage.agoMinutes", &[("min", &(s / 60))]),
        },
    };
    let (title, note) = if view.editing {
        (
            crate::i18n::t("pill.usage.customizeTitle"),
            crate::i18n::t("pill.usage.customizeHint").to_string(),
        )
    } else {
        (crate::i18n::t("pill.usage.title"), status)
    };
    if let Some(s) = m3 {
        return header_m3(title, note, view.editing, prefs.simple, narrow, s, cx);
    }
    let controls: AnyElement = if view.editing {
        div()
            .flex()
            .items_center()
            .gap(px(2.))
            .child(pill_fx(
                on_press(pill_button("usage-reset", crate::i18n::t("pill.customizeReset")), cx, |pill, _| pill.usage.reset_prefs()),
                "usage-reset",
                false,
                palette,
            ))
            .child(pill_fx(
                on_press(pill_button("usage-done", crate::i18n::t("pill.usage.done")), cx, |pill, _| pill.usage.set_editing(false)),
                "usage-done",
                true,
                palette,
            ))
            .into_any_element()
    } else {
        div()
            .flex()
            .items_center()
            .gap(px(2.))
            .child(pill_fx(
                on_press(pill_button("usage-simple", crate::i18n::t("pill.usage.simple")), cx, |pill, _| {
                    pill.usage.set_simple(true)
                }),
                "usage-simple",
                prefs.simple,
                palette,
            ))
            .child(pill_fx(
                on_press(pill_button("usage-detail", crate::i18n::t("pill.usage.detail")), cx, |pill, _| {
                    pill.usage.set_simple(false)
                }),
                "usage-detail",
                !prefs.simple,
                palette,
            ))
            .child(on_press(
                div()
                    .id("usage-edit")
                    .ml(px(4.))
                    .size(px(22.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(11.))
                    .cursor_pointer()
                    .tooltip(crate::hover::tip(crate::i18n::t("pill.usage.chooseAgents")))
                    .child(svg().path("icons/pencil.svg").size(px(13.)).text_color(palette.muted)),
                cx,
                |pill, _| pill.usage.set_editing(true),
            )
            .hover_bg("usage-edit-fx", palette.text.opacity(0.0), palette.text.opacity(0.12)))
            .into_any_element()
    };
    div()
        .h(px(HEADER_H))
        .px(px(SIDE + 4.))
        .flex()
        .items_center()
        .gap(px(8.))
        .child(
            div()
                .flex_none()
                .text_size(px(12.))
                .font_weight(FontWeight::SEMIBOLD)
                // Como el título de los otros vistazos (`peeks.rs`).
                .pill_text_em(12., type_scale::TITLE_MEDIUM)
                .text_color(palette.text)
                .child(title),
        )
        .child(
            div()
                .min_w_0()
                .truncate()
                .pill_text(10.5, type_scale::LABEL_SMALL)
                .text_color(palette.muted)
                .child(SharedString::from(note)),
        )
        .child(div().flex_1())
        .child(controls)
        .into_any_element()
}

/// El encabezado en Expressive, como el de los otros vistazos (`peeks.rs`):
/// el ícono en el acento y el título enfatizado; «Simple | Detalle» como un
/// grupo de botones conectado y el lápiz como botón de ícono estándar. En el
/// personalizador, «Restablecer» de texto y «Listo» lleno.
fn header_m3(
    title: &'static str,
    note: String,
    editing: bool,
    simple: bool,
    narrow: bool,
    s: Scheme,
    cx: &mut Context<Pill>,
) -> AnyElement {
    let controls: AnyElement = if editing {
        div()
            .flex()
            .items_center()
            .gap(px(4.))
            .child(
                on_press(text_button_m3("usage-reset", crate::i18n::t("pill.customizeReset")), cx, |pill, _| {
                    pill.usage.reset_prefs()
                })
                .fx_cx("usage-reset-fx", move |el, h, window, cx| {
                    let radius = crate::hover::m3_press_radius(h, HEADER_BTN_M3 / 2.0, 8.0, window, cx);
                    el.rounded(px(radius)).text_color(s.primary).bg(clear_layer(s.primary, h))
                }),
            )
            .child(
                on_press(text_button_m3("usage-done", crate::i18n::t("pill.usage.done")), cx, |pill, _| {
                    pill.usage.set_editing(false)
                })
                .fx_cx("usage-done-fx", move |el, h, window, cx| {
                    crate::hover::m3_tool(el, h, crate::hover::M3Tone::Filled, HEADER_BTN_M3, &s, window, cx).0
                }),
            )
            .into_any_element()
    } else {
        let segment = move |id: &'static str, index: usize, selected: bool, label: &'static str, to_simple: bool, cx: &mut Context<Pill>| {
            on_press(
                div()
                    .id(id)
                    .h(px(HEADER_BTN_M3))
                    .px(px(if narrow { 10. } else { 12. }))
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .gap(px(4.))
                    .type_style(if narrow { type_scale::LABEL_MEDIUM } else { type_scale::LABEL_LARGE })
                    .cursor_pointer(),
                cx,
                move |pill, _| pill.usage.set_simple(to_simple),
            )
            .fx_cx(SharedString::from(format!("{id}-fx")), move |el, h, window, cx| {
                let chosen = selected.then_some((s.secondary_container, s.on_secondary_container));
                let (el, fg) = crate::hover::m3_connected(el, h, (index, 2), chosen, HEADER_BTN_M3, &s, window, cx);
                // Elegido lleva el visto (sin espacio para él en el costado).
                el.when(selected && !narrow, |el| el.child(svg().path("icons/check.svg").size(px(14.)).flex_none().text_color(fg)))
                    .child(label)
            })
        };
        div()
            .flex()
            .items_center()
            .gap(px(4.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(2.))
                    .child(segment("usage-simple", 0, simple, crate::i18n::t("pill.usage.simple"), true, cx))
                    .child(segment("usage-detail", 1, !simple, crate::i18n::t("pill.usage.detail"), false, cx)),
            )
            .child(
                on_press(
                    div()
                        .id("usage-edit")
                        .size(px(ICON_BTN_M3))
                        .flex()
                        .flex_none()
                        .items_center()
                        .justify_center()
                        .cursor_pointer()
                        .tooltip(crate::hover::tip(crate::i18n::t("pill.usage.chooseAgents"))),
                    cx,
                    |pill, _| pill.usage.set_editing(true),
                )
                .fx_cx("usage-edit-fx", move |el, h, window, cx| {
                    let (el, fg) = crate::hover::m3_tool(el, h, crate::hover::M3Tone::Standard, ICON_BTN_M3, &s, window, cx);
                    el.child(svg().path("icons/pencil.svg").size(px(16.)).text_color(fg))
                }),
            )
            .into_any_element()
    };
    div()
        .h(px(HEADER_H))
        .px(px(SIDE + 4.))
        .flex()
        .items_center()
        .gap(px(8.))
        .when(!narrow, |el| {
            el.child(svg().path(crate::icons::themed("icons/gauge.svg")).size(px(16.)).flex_none().text_color(s.primary))
        })
        .child(
            div()
                .min_w_0()
                .truncate()
                .type_emphasized(if narrow { type_scale::TITLE_SMALL } else { type_scale::TITLE_MEDIUM })
                .text_color(s.on_surface)
                .child(title),
        )
        .child(
            div()
                .min_w_0()
                .truncate()
                .type_style(type_scale::LABEL_SMALL)
                .text_color(s.on_surface_variant)
                .child(SharedString::from(note)),
        )
        .child(div().flex_1())
        .child(div().flex_none().child(controls))
        .into_any_element()
}

/// Un botón de etiqueta del encabezado en Expressive (el color y la forma
/// los pone su `fx_cx`).
fn text_button_m3(id: &'static str, label: &'static str) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .h(px(HEADER_BTN_M3))
        .px(px(12.))
        .flex()
        .flex_none()
        .items_center()
        .type_style(type_scale::LABEL_LARGE)
        .cursor_pointer()
        .child(label)
}

/// La × de M3: un botón de ícono estándar con un área de clic de 40 en la
/// esquina; se ve el círculo de 28 con la capa de estado.
fn hide_button_m3(id: &'static str, fg: gpui::Hsla, cx: &mut Context<Pill>) -> AnyElement {
    on_press(
        div()
            .id(SharedString::from(format!("usage-hide-{id}")))
            .absolute()
            .top_0()
            .right_0()
            .size(px(40.))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer(),
        cx,
        move |pill, _| pill.usage.hide(id),
    )
    .fx(SharedString::from(format!("usage-hide-{id}-fx")), move |el, h| {
        el.child(
            div()
                .size(px(28.))
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .bg(clear_layer(fg, h))
                .child(svg().path("icons/x.svg").size(px(16.)).text_color(fg)),
        )
    })
    .into_any_element()
}

/// La × que aparece al pasar el cursor: quitar de la vista.
fn hide_button(id: &'static str, palette: &crate::Palette, cx: &mut Context<Pill>) -> AnyElement {
    on_press(
        div()
            .id(SharedString::from(format!("usage-hide-{id}")))
            .absolute()
            .top(px(5.))
            .right(px(5.))
            .size(px(18.))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(9.))
            .cursor_pointer()
            .child(svg().path("icons/x.svg").size(px(10.)).text_color(palette.text)),
        cx,
        move |pill, _| pill.usage.hide(id),
    )
    .hover_bg(
        SharedString::from(format!("usage-hide-{id}-fx")),
        palette.text.opacity(0.1),
        palette.text.opacity(0.22),
    )
    .into_any_element()
}

/// La × de la esquina aparece fundiéndose con el cursor sobre la tarjeta.
fn reveal_hide(hide: AnyElement, over: f32) -> impl IntoElement {
    div()
        .absolute()
        .inset_0()
        .opacity(over)
        .when(over < 0.05, |el| el.invisible())
        .child(hide)
}

/// La peor ventana vigente de un agente, en porcentaje.
fn worst(row: &Row) -> Option<f64> {
    row.quota
        .as_ref()
        .map(live_windows)
        .unwrap_or_default()
        .iter()
        // El uso extra se gasta recién cuando se acabó el plan: no es un tope.
        .filter(|w| w.kind != "extra")
        .map(|w| w.used_percent)
        .fold(None, |a: Option<f64>, p| Some(a.map_or(p, |a| a.max(p))))
}

/// Lo fijo de una tarjeta: el relleno, el nombre y la fila de etiquetas.
const CARD_PAD: f32 = 10.0;
const NAME_H: f32 = 16.0;
const TAGS_H: f32 = 14.0;
const CARD_GAP: f32 = 5.0;
/// Una ventana: etiqueta y números en una línea, la barra bajo ellos.
const WIN_LINE: f32 = 12.0;
const WIN_BAR: f32 = 4.0;
const WIN_H: f32 = WIN_LINE + 4.0 + WIN_BAR;
const WIN_GAP: f32 = 6.0;
/// Sin ventanas: el error ocupa hasta tres líneas.
const NOTE_LINES_H: f32 = 39.0;

/// Las ventanas de la tarjeta, todas y en el orden del proveedor (5 h,
/// semana, por modelo…): cada agente tiene las suyas y ninguna sobra.
fn card_windows(row: &Row) -> Vec<&quota::QuotaWindow> {
    row.quota.as_ref().map(live_windows).unwrap_or_default()
}

/// En Expressive las etiquetas son chips de asistencia de 24 y la línea de
/// cada ventana lleva texto de 16 (BODY_SMALL y LABEL_MEDIUM).
const TAGS_H_M3: f32 = 24.0;
const WIN_LINE_M3: f32 = 16.0;
const WIN_H_M3: f32 = WIN_LINE_M3 + 4.0 + WIN_BAR;

fn card_height_for(windows: usize) -> f32 {
    let m3 = crate::pill_look::content_scheme().is_some();
    let (tags, win) = if m3 { (TAGS_H_M3, WIN_H_M3) } else { (TAGS_H, WIN_H) };
    let body = if windows == 0 { NOTE_LINES_H } else { windows as f32 * win + (windows - 1) as f32 * WIN_GAP };
    CARD_PAD * 2.0 + NAME_H + CARD_GAP + tags + CARD_GAP + body
}

/// Crece con las ventanas del agente: OpenCode Go tiene tres, Claude
/// hasta cuatro o cinco, Cursor dos.
fn card_height(row: &Row) -> f32 {
    card_height_for(card_windows(row).len())
}

/// Las tarjetas de una línea van al alto de la más alta.
fn line_height(rows: &[Row]) -> f32 {
    rows.iter().map(card_height).fold(card_height_for(1), f32::max)
}

/// El anillo con la peor ventana y el logo al centro.
fn ring(row: &Row, size: f32, progress: f32, palette: &crate::Palette) -> gpui::Div {
    let worst = worst(row);
    let color = worst.map(tone).unwrap_or(palette.muted);
    // Se llena desde cero al abrir.
    let fraction = worst.map_or(0.0, |p| (p / 100.0) as f32) * progress;
    let track = palette.text.opacity(0.12);
    div()
        .relative()
        .size(px(size))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .child(
            canvas(
                |_, _, _| {},
                move |bounds, _, window, _| {
                    let c = bounds.center();
                    let (cx, cy) = (f32::from(c.x), f32::from(c.y));
                    let r = size / 2.0 - 3.0;
                    if let Some(path) = arc(cx, cy, r, 1.0, 3.0) {
                        window.paint_path(path, track);
                    }
                    if worst.is_some() && progress > 0.02 {
                        if let Some(path) = arc(cx, cy, r, fraction.max(0.01), 3.0) {
                            window.paint_path(path, color);
                        }
                    }
                },
            )
            .absolute()
            .inset_0(),
        )
        .child(svg().path(row.family.logo).size(px(size * 0.4)).text_color(palette.text))
}

/// Un trazo de arco de M3 desde `from`, `sweep` radianes en el sentido del
/// reloj; con `caps`, extremos redondeados.
#[allow(clippy::too_many_arguments)]
fn stroke_arc(
    (cx, cy, r): (f32, f32, f32),
    from: f32,
    sweep: f32,
    width: f32,
    color: gpui::Hsla,
    caps: bool,
    window: &mut gpui::Window,
) {
    let steps = ((sweep * r / 1.5).ceil() as usize).max(2);
    let points: Vec<_> = (0..=steps)
        .map(|i| {
            let a = from + sweep * i as f32 / steps as f32;
            gpui::point(px(cx + r * a.cos()), px(cy + r * a.sin()))
        })
        .collect();
    let cap = if caps { lyon::tessellation::LineCap::Round } else { lyon::tessellation::LineCap::Butt };
    let options = lyon::tessellation::StrokeOptions::default()
        .with_line_width(width)
        .with_line_cap(cap)
        .with_line_join(lyon::tessellation::LineJoin::Round);
    let mut builder = gpui::PathBuilder::stroke(px(width)).with_style(gpui::PathStyle::Stroke(options));
    builder.add_polygon(&points, false);
    if let Ok(path) = builder.build() {
        window.paint_path(path, color);
    }
}

/// El anillo como CircularProgress de M3 (2024): trazo de 4 con extremos
/// redondeados, y la pista separada del indicador por un hueco. El indicador
/// en el acento (o `tertiary`/`error` al pasar los umbrales) y el logo al
/// centro. Se dibuja en el `paint`: quieto, no pide cuadros.
fn ring_m3(row: &Row, size: f32, progress: f32, track: gpui::Hsla, logo: gpui::Hsla, s: &Scheme) -> gpui::Div {
    const STROKE: f32 = 4.0;
    const HOLE: f32 = 4.0;
    let worst = worst(row);
    let color = worst.map_or(s.on_surface_variant, |p| m3_tone(p, s));
    // Se llena desde cero al abrir.
    let fraction = worst.map_or(0.0, |p| (p / 100.0).clamp(0.0, 1.0) as f32) * progress;
    div()
        .relative()
        .size(px(size))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .child(
            canvas(
                |_, _, _| {},
                move |bounds, _, window, _| {
                    use std::f32::consts::{FRAC_PI_2, TAU};
                    let c = bounds.center();
                    let circle = (f32::from(c.x), f32::from(c.y), size / 2.0 - STROKE / 2.0);
                    let sweep = TAU * fraction;
                    if sweep < 0.01 {
                        stroke_arc(circle, -FRAC_PI_2, TAU, STROKE, track, false, window);
                        return;
                    }
                    // El hueco cuenta los medios trazos de los extremos redondos.
                    let hole = (HOLE + STROKE) / circle.2;
                    let rest = TAU - sweep - 2.0 * hole;
                    if rest > 0.01 {
                        stroke_arc(circle, -FRAC_PI_2 + sweep + hole, rest, STROKE, track, true, window);
                    }
                    stroke_arc(circle, -FRAC_PI_2, sweep, STROKE, color, true, window);
                },
            )
            .absolute()
            .inset_0(),
        )
        .child(svg().path(row.family.logo).size(px(size * 0.4)).text_color(logo))
}

/// La barra como LinearProgress de M3 (2024): trazo de 4 redondeado, el
/// indicador, un hueco, la pista y el punto de tope al final.
fn meter_m3(fraction: f32, color: gpui::Hsla, track: gpui::Hsla) -> gpui::Div {
    let fraction = fraction.clamp(0.0, 1.0);
    div()
        .h(px(WIN_BAR))
        .flex()
        .gap(px(4.))
        .when(fraction > 0.005, |el| {
            el.child(div().h_full().w(gpui::relative(fraction)).rounded_full().bg(color))
        })
        .when(fraction < 0.995, |el| {
            el.child(
                div()
                    .flex_1()
                    .min_w(px(WIN_BAR))
                    .h_full()
                    .flex()
                    .justify_end()
                    .rounded_full()
                    .bg(track)
                    .child(div().size(px(WIN_BAR)).flex_none().rounded_full().bg(color)),
            )
        })
}

/// En modo simple y Expressive: una tarjeta seleccionable de M3 con el anillo,
/// el porcentaje y el nombre. La elegida va en `secondary_container` y se
/// vuelve más cuadrada con un resorte; al apretar, más todavía.
fn simple_item_m3(row: &Row, focused: bool, progress: f32, s: Scheme, cx: &mut Context<Pill>) -> AnyElement {
    let id = row.family.id;
    let worst = worst(row);
    let group = SharedString::from(format!("usage-simple-{id}"));
    let (base, fg, sub, track) = if focused {
        (s.secondary_container, s.on_secondary_container, s.on_secondary_container, s.surface_container_highest)
    } else {
        (s.surface_container_high, s.on_surface, s.on_surface_variant, s.secondary_container)
    };
    let hide = hide_button_m3(id, sub, cx);
    div()
        .id(group.clone())
        .relative()
        .flex_1()
        .min_w_0()
        .h_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(2.))
        .cursor_pointer()
        .on_hover(cx.listener(move |pill, hovered: &bool, _, cx| {
            if *hovered && !pill.usage.view().editing {
                pill.usage.set_focus(Some(id));
                cx.notify();
            }
        }))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |pill, _: &MouseDownEvent, _, cx| {
                pill.usage.press(id, cx);
                cx.stop_propagation();
            }),
        )
        // Entra deslizándose un poco hacia arriba, con fundido.
        .top(px(8.0 * (1.0 - progress)))
        .opacity(progress)
        .child(ring_m3(row, RING, progress, track, fg, &s))
        .child(
            div()
                .map(percent_text)
                .text_color(fg)
                // El porcentaje cuenta hacia arriba mientras el anillo se llena.
                .child(SharedString::from(worst.map_or("—".into(), |p| format!("{:.0} %", p * progress as f64)))),
        )
        .child(
            div()
                .max_w_full()
                .px(px(6.))
                .truncate()
                .type_style(type_scale::LABEL_MEDIUM)
                .line_height(px(14.))
                .text_color(sub)
                .child(row.family.name),
        )
        .fx_cx(SharedString::from(format!("{group}-fx")), move |el, h, window, cx| {
            let target = if h.press > 0.5 {
                f32::from(corner::S)
            } else if focused {
                f32::from(corner::M)
            } else {
                f32::from(corner::LI)
            };
            let radius = gpui_m3::motion::animate_f32("usage-item-radius", Spring::SPATIAL, target, window, cx).max(0.0);
            el.rounded(px(radius)).bg(m3_layer(base, fg, h)).child(reveal_hide(hide, h.over))
        })
        .into_any_element()
}

/// En modo simple: el anillo, el porcentaje y el nombre. Al pasar o
/// presionar baja su tarjeta; mantener presionado abre el personalizador.
fn simple_item(row: &Row, focused: bool, progress: f32, palette: &crate::Palette, cx: &mut Context<Pill>) -> AnyElement {
    let id = row.family.id;
    let worst = worst(row);
    let group = SharedString::from(format!("usage-simple-{id}"));
    let text = palette.text;
    let hide = hide_button(id, palette, cx);
    div()
        .id(group.clone())
        .relative()
        .flex_1()
        .min_w_0()
        .h_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(2.))
        .rounded(px(12.))
        .cursor_pointer()
        .on_hover(cx.listener(move |pill, hovered: &bool, _, cx| {
            if *hovered && !pill.usage.view().editing {
                pill.usage.set_focus(Some(id));
                cx.notify();
            }
        }))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |pill, _: &MouseDownEvent, _, cx| {
                pill.usage.press(id, cx);
                cx.stop_propagation();
            }),
        )
        // Entra deslizándose un poco hacia arriba, con fundido.
        .top(px(8.0 * (1.0 - progress)))
        .opacity(progress)
        .child(ring(row, RING, progress, palette))
        .child(
            div()
                .text_size(px(11.5))
                .font_weight(FontWeight::SEMIBOLD)
                .map(percent_text)
                .text_color(worst.map(tone).unwrap_or(palette.muted))
                // El porcentaje cuenta hacia arriba mientras el anillo se llena.
                .child(SharedString::from(worst.map_or("—".into(), |p| format!("{:.0} %", p * progress as f64)))),
        )
        // El nombre, chico: algunos logos se parecen (Antigravity y Grok).
        .child(div().max_w_full().truncate().pill_text(9.5, type_scale::LABEL_SMALL).text_color(palette.muted).child(row.family.name))
        .fx(SharedString::from(format!("{group}-fx")), move |el, h| {
            el.bg(h.mix(text.opacity(0.0), text.opacity(0.08))).child(reveal_hide(hide, h.over))
        })
        .lit(focused)
        .into_any_element()
}

/// La tarjeta de un agente: anillo, nombre, lo instalado con el plan (o la
/// edad del dato) y una barra por ventana. La × de la esquina la saca de la
/// vista; mantenerla presionada abre el personalizador.
fn card(row: &Row, height: f32, progress: f32, palette: &crate::Palette, m3: Option<Scheme>, cx: &mut Context<Pill>) -> AnyElement {
    let id = row.family.id;
    let windows = card_windows(row);
    let worst = worst(row);

    let quota = row.quota.as_ref();
    let stale = quota.and_then(|q| q.fetched_at).filter(|at| now_ms() - at > 15 * 60_000);
    let side = match (stale, quota.and_then(|q| q.plan.clone())) {
        (Some(at), _) => crate::i18n::tf("pill.quota.stale", &[("when", &span(now_ms() - at))]),
        (None, Some(plan)) => plan,
        (None, None) => String::new(),
    };
    let tags_h = if m3.is_some() { TAGS_H_M3 } else { TAGS_H };
    let tag = |(icon, text): (Option<&'static str>, String)| match m3 {
        // Un chip de asistencia de M3: contorno, ícono en el acento y etiqueta.
        Some(s) => div()
            .flex_none()
            .h(px(TAGS_H_M3))
            .px(px(8.))
            .flex()
            .items_center()
            .gap(px(4.))
            .rounded(corner::S)
            .border_1()
            .border_color(s.outline_variant)
            .type_style(type_scale::LABEL_MEDIUM)
            .text_color(s.on_surface)
            .children(icon.map(|icon| svg().path(crate::icons::themed(icon)).size(px(14.)).flex_none().text_color(s.primary)))
            .child(SharedString::from(text)),
        None => div()
            .flex_none()
            .h(px(TAGS_H))
            .line_height(px(TAGS_H))
            .px(px(6.))
            .rounded(px(6.))
            .bg(palette.text.opacity(0.07))
            .pill_text(9.5, type_scale::LABEL_SMALL)
            .line_height(px(TAGS_H))
            .text_color(palette.text.opacity(0.7))
            .child(SharedString::from(text))
    };
    let mut tags: Vec<(Option<&'static str>, String)> = Vec::new();
    if row.installed.cli {
        tags.push((Some("icons/square-terminal.svg"), crate::i18n::t("page.agents.window.shellLabel").into()));
    }
    for app in &row.installed.apps {
        tags.push((
            None,
            if *app == row.family.name { crate::i18n::t("pill.usage.app").into() } else { format!("{} · {app}", crate::i18n::t("pill.usage.app")) },
        ));
    }

    let body: AnyElement = match quota {
        Some(_) if !windows.is_empty() => div()
            .flex()
            .flex_col()
            .gap(px(WIN_GAP))
            .children(windows.iter().map(|w| match m3 {
                Some(s) => bar_m3(w, progress, s),
                None => bar(w, progress, palette),
            }))
            .into_any_element(),
        Some(q) if q.spend.is_some() => {
            let spend = q.spend.as_ref().map(|s| s.cents / 100.0).unwrap_or_default();
            div()
                .pill_text(11., type_scale::BODY_SMALL)
                .text_color(palette.text.opacity(0.8))
                .child(SharedString::from(crate::i18n::tf("pill.quota.spend", &[("amount", &format!("{spend:.2}"))])))
                .into_any_element()
        }
        // Expressive: 11 px con el peso del cuerpo; las tres líneas de 13
        // llenan `NOTE_LINES_H` y con 12 px se cortarían.
        Some(q) => div()
            .pill_text(10.5, type_scale::LABEL_SMALL)
            .font_weight(FontWeight::NORMAL)
            .line_height(px(13.))
            .line_clamp(3)
            .text_color(m3.map_or(gpui::Hsla::from(gpui::rgb(WARN)).opacity(0.85), |s| s.error))
            .child(SharedString::from(q.error.clone().unwrap_or_else(|| crate::i18n::t("pill.usage.noQuota").into())))
            .into_any_element(),
        None => div()
            .pill_text(10.5, type_scale::BODY_SMALL)
            .text_color(palette.muted)
            .child(crate::i18n::t("settings.agents.mcpSignedOut"))
            .into_any_element(),
    };

    let group = SharedString::from(format!("usage-card-{id}"));
    let text = palette.text;
    let hide = match m3 {
        Some(s) => hide_button_m3(id, s.on_surface_variant, cx),
        None => hide_button(id, palette, cx),
    };
    let ring_el = match m3 {
        Some(s) => ring_m3(row, RING, progress, s.secondary_container, s.on_surface, &s),
        None => ring(row, RING, progress, palette),
    };
    div()
        .id(group.clone())
        .relative()
        .h(px(height))
        .p(px(CARD_PAD))
        .when(m3.is_none(), |el| el.rounded(px(14.)))
        .flex()
        .gap(px(10.))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |pill, _: &MouseDownEvent, _, cx| {
                pill.usage.press(id, cx);
                cx.stop_propagation();
            }),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .items_center()
                .gap(px(3.))
                .child(ring_el)
                .child(
                    div()
                        .text_size(px(11.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .map(percent_text)
                        // En Expressive la cifra va en el texto; el color del
                        // umbral ya lo lleva el anillo.
                        .text_color(match m3 {
                            Some(s) => s.on_surface,
                            None => worst.map(tone).unwrap_or(palette.muted),
                        })
                        .child(SharedString::from(worst.map_or("—".into(), |p| format!("{:.0} %", p * progress as f64)))),
                ),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(CARD_GAP))
                .child(
                    div()
                        .h(px(NAME_H))
                        .text_size(px(12.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .pill_text_em(12.5, type_scale::TITLE_SMALL)
                        .line_height(px(NAME_H))
                        .text_color(palette.text)
                        .child(row.family.name),
                )
                .child(
                    div()
                        .h(px(tags_h))
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .children(tags.into_iter().map(tag))
                        .child(div().flex_1())
                        .child(
                            div()
                                .min_w_0()
                                .truncate()
                                .pill_text(10., type_scale::LABEL_MEDIUM)
                                .line_height(px(tags_h))
                                .text_color(palette.muted)
                                .child(SharedString::from(side)),
                        ),
                )
                .child(body),
        )
        .fx_cx(SharedString::from(format!("{group}-fx")), move |el, h, window, cx| {
            let el = match m3 {
                // Una tarjeta de M3 un escalón sobre la pill, con capa de estado
                // y el radio que se cuadra al apretar.
                Some(s) => {
                    let radius = crate::hover::m3_press_radius(h, f32::from(corner::L), f32::from(corner::M), window, cx);
                    el.rounded(px(radius)).bg(m3_layer(s.surface_container_high, s.on_surface, h))
                }
                None => el.bg(h.mix(text.opacity(0.055), text.opacity(0.085))),
            };
            el.child(reveal_hide(hide, h.over))
        })
        .into_any_element()
}

/// Una ventana en Expressive: la etiqueta en BODY_SMALL y, a la derecha, el
/// porcentaje y cuándo se reinicia en LABEL_MEDIUM tabular; bajo ellos la
/// barra de M3.
fn bar_m3(window: &quota::QuotaWindow, progress: f32, s: Scheme) -> AnyElement {
    let pct = window.used_percent.clamp(0.0, 100.0);
    let reset = window.resets_at.map(|at| span(at - now_ms())).unwrap_or_default();
    div()
        .h(px(WIN_H_M3))
        .flex()
        .flex_col()
        .justify_between()
        .child(
            div()
                .h(px(WIN_LINE_M3))
                .flex()
                .items_center()
                .gap(px(6.))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .type_style(type_scale::BODY_SMALL)
                        .text_color(s.on_surface_variant)
                        .child(SharedString::from(window_label(&window.kind, window.minutes))),
                )
                .child(
                    div()
                        .flex_none()
                        .map(|el| tabular_text(el, type_scale::LABEL_MEDIUM))
                        .text_color(s.on_surface)
                        .child(SharedString::from(if reset.is_empty() {
                            format!("{pct:.0} %")
                        } else {
                            format!("{pct:.0} % · {reset}")
                        })),
                ),
        )
        // Crece desde cero junto con el anillo.
        .child(meter_m3((pct / 100.0) as f32 * progress, m3_tone(pct, &s), s.secondary_container))
        .into_any_element()
}

/// Una ventana: la etiqueta entera y, a la derecha, el porcentaje y cuándo
/// se reinicia; bajo ellos la barra, a todo el ancho de la tarjeta.
fn bar(window: &quota::QuotaWindow, progress: f32, palette: &crate::Palette) -> AnyElement {
    let pct = window.used_percent.clamp(0.0, 100.0);
    let reset = window.resets_at.map(|at| span(at - now_ms())).unwrap_or_default();
    div()
        .h(px(WIN_H))
        .flex()
        .flex_col()
        .justify_between()
        .child(
            div()
                .h(px(WIN_LINE))
                .flex()
                .items_center()
                .gap(px(6.))
                .pill_text(10., type_scale::LABEL_SMALL)
                .line_height(px(WIN_LINE))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_color(palette.muted)
                        .child(SharedString::from(window_label(&window.kind, window.minutes))),
                )
                .child(
                    div()
                        .flex_none()
                        .text_color(palette.text.opacity(0.85))
                        .child(SharedString::from(if reset.is_empty() {
                            format!("{pct:.0} %")
                        } else {
                            format!("{pct:.0} % · {reset}")
                        })),
                ),
        )
        .child(
            div().h(px(WIN_BAR)).rounded(px(2.)).bg(palette.text.opacity(0.12)).child(
                div()
                    .h_full()
                    .rounded(px(2.))
                    .bg(tone(pct))
                    // Crece desde cero junto con el anillo.
                    .w(gpui::relative((pct.max(2.0) / 100.0) as f32 * progress.max(0.02))),
            ),
        )
        .into_any_element()
}

/// Lo que se ve de una ficha del personalizador.
fn chip_face(row: &Row, shown: bool, width: f32, palette: &crate::Palette, m3: Option<Scheme>) -> gpui::Div {
    if let Some(s) = m3 {
        // En Expressive, como un chip de filtro: a la vista en
        // `secondary_container`; fuera, con contorno.
        let (bg, fg, icon) = if shown {
            (s.secondary_container, s.on_secondary_container, s.on_secondary_container)
        } else {
            (s.surface_container_high, s.on_surface_variant, s.on_surface_variant)
        };
        return div()
            .w(px(width))
            .h(px(CHIP_H))
            .px(px(10.))
            .flex()
            .items_center()
            .gap(px(8.))
            .rounded(corner::S)
            .bg(bg)
            .when(!shown, |el| el.border_1().border_color(s.outline_variant))
            .child(svg().path(row.family.logo).size(px(16.)).flex_none().text_color(icon))
            .child(div().flex_1().truncate().type_style(type_scale::LABEL_LARGE).text_color(fg).child(row.family.name));
    }
    div()
        .w(px(width))
        .h(px(CHIP_H))
        .px(px(10.))
        .flex()
        .items_center()
        .gap(px(8.))
        .rounded(px(10.))
        .bg(palette.text.opacity(if shown { 0.12 } else { 0.05 }))
        .child(
            svg()
                .path(row.family.logo)
                .size(px(14.))
                .text_color(if shown { palette.text } else { palette.text.opacity(0.5) }),
        )
        .child(
            div()
                .flex_1()
                .truncate()
                .pill_text(12., type_scale::LABEL_LARGE)
                .text_color(if shown { palette.text } else { palette.muted })
                .child(row.family.name),
        )
}

/// El personalizador: «A la vista» y «Fuera de la vista».
fn editor(
    snap: &Snapshot,
    prefs: &Prefs,
    view: &View,
    usage: &Usage,
    chip_w: f32,
    palette: &crate::Palette,
    m3: Option<Scheme>,
    cx: &mut Context<Pill>,
) -> AnyElement {
    let (shown, out) = arranged(snap, prefs);
    let drag = view.drag.filter(|d| d.moved);
    let lane = |hidden: bool, rows: Vec<Row>, cx: &mut Context<Pill>| {
        let title = if hidden { crate::i18n::t("pill.usage.laneHidden") } else { crate::i18n::t("pill.usage.laneShown") };
        let count = rows.len();
        // La ficha arrastrada sale de su fila; donde caería queda un hueco.
        let mut cells: Vec<Option<Row>> =
            rows.into_iter().filter(|r| drag.is_none_or(|d| d.id != r.family.id)).map(Some).collect();
        if let Some((h, index)) = drag.and_then(|d| d.target) {
            if h == hidden {
                cells.insert(index.min(cells.len()), None);
            }
        }
        let empty = cells.is_empty();
        let store = usage.clone();
        div()
            .relative()
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(LANE_LABEL))
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .pill_text(10.5, type_scale::LABEL_MEDIUM)
                    .text_color(palette.muted)
                    .child(title)
                    .child(
                        div()
                            .text_color(m3.map_or(palette.text.opacity(0.4), |s| s.on_surface_variant))
                            .child(SharedString::from(count.to_string())),
                    ),
            )
            .child(
                div()
                    .relative()
                    .min_h(px(CHIP_H))
                    .flex()
                    .flex_wrap()
                    .gap(px(CHIP_GAP))
                    .child(
                        canvas(
                            move |bounds: Bounds<Pixels>, _, _| {
                                store.update_view(|v| v.lanes[hidden as usize] = Some(bounds));
                            },
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .inset_0(),
                    )
                    .children(cells.into_iter().map(|cell| match cell {
                        None => div()
                            .w(px(chip_w))
                            .h(px(CHIP_H))
                            .rounded(m3.map_or(px(10.), |_| corner::S))
                            .border_1()
                            .border_color(m3.map_or(palette.text.opacity(0.25), |s| s.outline))
                            .into_any_element(),
                        Some(row) => {
                            let id = row.family.id;
                            let store = usage.clone();
                            div()
                                .id(SharedString::from(format!("usage-chip-{id}")))
                                .relative()
                                .cursor_grab()
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(move |pill, event: &MouseDownEvent, _, cx| {
                                        pill.usage.grab(id, (f32::from(event.position.x), f32::from(event.position.y)));
                                        cx.stop_propagation();
                                        cx.notify();
                                    }),
                                )
                                .child(
                                    canvas(
                                        move |bounds: Bounds<Pixels>, _, _| {
                                            store.update_view(|v| v.chips.push((id, hidden, bounds)));
                                        },
                                        |_, _, _, _| {},
                                    )
                                    .absolute()
                                    .inset_0(),
                                )
                                .child(chip_face(&row, !hidden, chip_w, palette, m3))
                                .fx(SharedString::from(format!("usage-chip-{id}-fx")), |el, h| {
                                    el.opacity(1.0 - 0.15 * h.t)
                                })
                                .into_any_element()
                        }
                    }))
                    .when(empty, |el| {
                        el.child(
                            div()
                                .h(px(CHIP_H))
                                .flex()
                                .items_center()
                                .pill_text(11., type_scale::BODY_SMALL)
                                .text_color(m3.map_or(palette.text.opacity(0.35), |s| s.on_surface_variant))
                                .child(crate::i18n::t("pill.usage.dropHere")),
                        )
                    }),
            )
    };
    div()
        .px(px(SIDE))
        .flex()
        .flex_col()
        .gap(px(LANE_GAP))
        .child(lane(false, shown, cx))
        .child(lane(true, out, cx))
        .when(view.refused, |el| {
            el.child(
                div()
                    .h(px(NOTE_H - LANE_GAP).max(px(10.)))
                    .pill_text(10.5, type_scale::BODY_SMALL)
                    .text_color(m3.map_or(gpui::Hsla::from(gpui::rgb(WARN)), |s| s.error))
                    .child(crate::i18n::t("pill.usage.keepOne")),
            )
        })
        .into_any_element()
}

/// «Abrir Atic Code» al pie del vistazo, en Expressive: el botón tonal de M3
/// de los otros vistazos («Abrir …» de `peeks.rs`): `secondary_container`,
/// LABEL_LARGE, la flecha y la capa de estado.
pub(crate) fn open_code_m3(
    id: &'static str,
    s: Scheme,
    on_click: impl Fn(&gpui::ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static,
) -> AnyElement {
    let fg = s.on_secondary_container;
    div()
        .id(id)
        .h(px(28.))
        .px(px(12.))
        .flex()
        .flex_none()
        .items_center()
        .gap(px(4.))
        .rounded_full()
        .type_style(type_scale::LABEL_LARGE)
        .text_color(fg)
        .cursor_pointer()
        .on_click(on_click)
        .fx(SharedString::from(format!("{id}-fx")), move |el, h| {
            el.bg(m3_layer(s.secondary_container, fg, h))
                .child(crate::i18n::t("pill.agents.openCode"))
                .child(svg().path(crate::icons::themed("icons/arrow-up-right.svg")).size(px(11.)).text_color(fg).ml(px(2.0 * h.t)))
        })
        .into_any_element()
}
