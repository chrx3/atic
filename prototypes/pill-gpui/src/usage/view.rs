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
use crate::hover::HoverExt;

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
        let snap = self.usage.snapshot();
        let prefs = self.usage.prefs();
        let view = self.usage.view();
        let narrow = self.side_drawers();
        if view.editing {
            let width = if narrow { crate::SIDE_W } else { crate::strip_open_length().max(PANEL_W) };
            let per = per_line(width);
            let (shown, out) = arranged(&snap, &prefs);
            let note = if view.refused { NOTE_H } else { 0.0 };
            return HEADER_H + lane_height(shown.len(), per) + LANE_GAP + lane_height(out.len(), per) + note + BOTTOM;
        }
        let shown = visible(&snap, &prefs);
        if prefs.simple {
            let detail = if view.focus.is_some() { GAP + CARD_H } else { 0.0 };
            let rows = shown.len().max(1).div_ceil(per_row(narrow, true)) as f32;
            return HEADER_H + rows * SIMPLE_H + (rows - 1.0) * RING_ROW_GAP + detail + BOTTOM;
        }
        let lines = shown.len().max(1).div_ceil(per_row(narrow, false)) as f32;
        HEADER_H + lines * CARD_H + (lines - 1.0) * GAP + BOTTOM
    }

    /// El cursor sobre el vistazo de uso (la bandeja lo cuenta como suyo).
    pub(crate) fn usage_over(&self, p: (f32, f32), now: Instant) -> bool {
        self.usage_rect(now).is_some_and(|r| r.contains(p, 4.0))
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
        let narrow = self.side_drawers();
        let palette = crate::Palette::dark();
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
            editor(&snap, &prefs, &view, &self.usage, chip_width(area.w), &palette, cx)
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
                                .gap(px(4.))
                                .children(chunk.iter().enumerate().map(|(j, row)| {
                                    let i = line.saturating_mul(per) + j;
                                    simple_item(row, view.focus == Some(row.family.id), enter(i), &palette, cx)
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
                        .child(card(row, focused.max(enter(0)), &palette, cx))
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
                                .child(card(row, p, &palette, cx))
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
                    .child(chip_face(&row, true, chip_width(area.w), &palette).opacity(0.92)),
            )
        });

        Some(
            div()
                .id("usage-peek")
                .absolute()
                .left(px(area.x))
                .top(px(area.y))
                .w(px(area.w))
                .h(px(area.h))
                .overflow_hidden()
                // Sin fondo propio: el tab entero se vuelve casi opaco
                // (`CONTENT_TINT`), como con la letra.
                .opacity(segment(amount, 0.55, 0.45))
                .font_family("Segoe UI")
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
                .child(header(&snap, self.usage.loading(), &prefs, &view, &palette, cx))
                .child(div().opacity(switched).child(body))
                .children(ghost)
                .into_any_element(),
        )
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

/// Un botón chico del encabezado.
fn pill_button(id: &'static str, label: &str) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .h(px(22.))
        .px(px(9.))
        .flex()
        .items_center()
        .rounded(px(11.))
        .text_size(px(11.))
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
    palette: &crate::Palette,
    cx: &mut Context<Pill>,
) -> AnyElement {
    let status = match snap.at {
        None => "Leyendo…".to_string(),
        Some(_) if loading => "Actualizando…".to_string(),
        Some(at) => match at.elapsed().as_secs() {
            0..=4 => "recién".to_string(),
            s if s < 60 => format!("hace {s} s"),
            s => format!("hace {} min", s / 60),
        },
    };
    let (title, note) = if view.editing {
        ("Personalizar el uso", "Arrastra para ordenar · clic para mover de fila".to_string())
    } else {
        ("Uso de agentes", status)
    };
    let controls: AnyElement = if view.editing {
        div()
            .flex()
            .items_center()
            .gap(px(2.))
            .child(pill_fx(
                on_press(pill_button("usage-reset", "Restablecer"), cx, |pill, _| pill.usage.reset_prefs()),
                "usage-reset",
                false,
                palette,
            ))
            .child(pill_fx(
                on_press(pill_button("usage-done", "Listo"), cx, |pill, _| pill.usage.set_editing(false)),
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
                on_press(pill_button("usage-simple", "Simple"), cx, |pill, _| {
                    pill.usage.set_simple(true)
                }),
                "usage-simple",
                prefs.simple,
                palette,
            ))
            .child(pill_fx(
                on_press(pill_button("usage-detail", "Detalle"), cx, |pill, _| {
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
                    .tooltip(crate::hover::tip("Elegir qué agentes se ven"))
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
                .text_color(palette.text)
                .child(title),
        )
        .child(
            div()
                .min_w_0()
                .truncate()
                .text_size(px(10.5))
                .text_color(palette.muted)
                .child(SharedString::from(note)),
        )
        .child(div().flex_1())
        .child(controls)
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
        .map(|w| w.used_percent)
        .fold(None, |a: Option<f64>, p| Some(a.map_or(p, |a| a.max(p))))
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
                .text_color(worst.map(tone).unwrap_or(palette.muted))
                // El porcentaje cuenta hacia arriba mientras el anillo se llena.
                .child(SharedString::from(worst.map_or("—".into(), |p| format!("{:.0} %", p * progress as f64)))),
        )
        // El nombre, chico: algunos logos se parecen (Antigravity y Grok).
        .child(div().max_w_full().truncate().text_size(px(9.5)).text_color(palette.muted).child(row.family.name))
        .fx(SharedString::from(format!("{group}-fx")), move |el, h| {
            el.bg(h.mix(text.opacity(0.0), text.opacity(0.08))).child(reveal_hide(hide, h.over))
        })
        .lit(focused)
        .into_any_element()
}

/// La tarjeta de un agente: anillo, nombre, lo instalado con el plan (o la
/// edad del dato) y una barra por ventana. La × de la esquina la saca de la
/// vista; mantenerla presionada abre el personalizador.
fn card(row: &Row, progress: f32, palette: &crate::Palette, cx: &mut Context<Pill>) -> AnyElement {
    let id = row.family.id;
    // Las más usadas primero: las barras que caben son las que importan.
    let mut windows: Vec<&quota::QuotaWindow> = row.quota.as_ref().map(live_windows).unwrap_or_default();
    windows.sort_by(|a, b| b.used_percent.total_cmp(&a.used_percent));
    let worst = worst(row);

    let quota = row.quota.as_ref();
    let stale = quota.and_then(|q| q.fetched_at).filter(|at| now_ms() - at > 15 * 60_000);
    let side = match (stale, quota.and_then(|q| q.plan.clone())) {
        (Some(at), _) => format!("hace {}", span(now_ms() - at)),
        (None, Some(plan)) => plan,
        (None, None) => String::new(),
    };
    let tag = |text: String| {
        div()
            .flex_none()
            .px(px(6.))
            .rounded(px(6.))
            .bg(palette.text.opacity(0.07))
            .text_size(px(9.5))
            .text_color(palette.text.opacity(0.7))
            .child(SharedString::from(text))
    };
    let mut tags: Vec<String> = Vec::new();
    if row.installed.cli {
        tags.push("Terminal".into());
    }
    for app in &row.installed.apps {
        tags.push(if *app == row.family.name { "App".into() } else { format!("App · {app}") });
    }

    let body: AnyElement = match quota {
        Some(_) if !windows.is_empty() => div()
            .flex()
            .flex_col()
            .gap(px(5.))
            .children(windows.iter().take(2).map(|w| bar(w, progress, palette)))
            .into_any_element(),
        Some(q) if q.spend.is_some() => {
            let spend = q.spend.as_ref().map(|s| s.cents / 100.0).unwrap_or_default();
            div()
                .text_size(px(11.))
                .text_color(palette.text.opacity(0.8))
                .child(SharedString::from(format!("US$ {spend:.2} este período")))
                .into_any_element()
        }
        Some(q) => div()
            .text_size(px(10.5))
            .line_height(px(13.))
            .line_clamp(3)
            .text_color(gpui::Hsla::from(gpui::rgb(WARN)).opacity(0.85))
            .child(SharedString::from(q.error.clone().unwrap_or_else(|| "Sin cupo publicado".into())))
            .into_any_element(),
        None => div()
            .text_size(px(10.5))
            .text_color(palette.muted)
            .child("Sin sesión iniciada")
            .into_any_element(),
    };

    let group = SharedString::from(format!("usage-card-{id}"));
    let text = palette.text;
    let hide = hide_button(id, palette, cx);
    div()
        .id(group.clone())
        .relative()
        .h(px(CARD_H))
        .p(px(10.))
        .rounded(px(14.))
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
                .child(ring(row, RING, progress, palette))
                .child(
                    div()
                        .text_size(px(11.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(worst.map(tone).unwrap_or(palette.muted))
                        .child(SharedString::from(worst.map_or("—".into(), |p| format!("{:.0} %", p * progress as f64)))),
                ),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(5.))
                .child(
                    div()
                        .text_size(px(12.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(palette.text)
                        .child(row.family.name),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .children(tags.into_iter().map(tag))
                        .child(div().flex_1())
                        .child(
                            div()
                                .min_w_0()
                                .truncate()
                                .text_size(px(10.))
                                .text_color(palette.muted)
                                .child(SharedString::from(side)),
                        ),
                )
                .child(body),
        )
        .fx(SharedString::from(format!("{group}-fx")), move |el, h| {
            el.bg(h.mix(text.opacity(0.055), text.opacity(0.085))).child(reveal_hide(hide, h.over))
        })
        .into_any_element()
}

/// Una ventana: etiqueta, barra fina, porcentaje y cuándo se reinicia.
fn bar(window: &quota::QuotaWindow, progress: f32, palette: &crate::Palette) -> AnyElement {
    let pct = window.used_percent.clamp(0.0, 100.0);
    let reset = window.resets_at.map(|at| span(at - now_ms())).unwrap_or_default();
    div()
        .flex()
        .items_center()
        .gap(px(6.))
        .text_size(px(10.))
        .child(
            div()
                .w(px(40.))
                .flex_none()
                .truncate()
                .text_color(palette.muted)
                .child(SharedString::from(window_label(&window.kind, window.minutes))),
        )
        .child(
            div().flex_1().h(px(4.)).rounded(px(2.)).bg(palette.text.opacity(0.12)).child(
                div()
                    .h_full()
                    .rounded(px(2.))
                    .bg(tone(pct))
                    // Crece desde cero junto con el anillo.
                    .w(gpui::relative((pct.max(2.0) / 100.0) as f32 * progress.max(0.02))),
            ),
        )
        .child(
            div()
                .flex_none()
                .text_color(palette.text.opacity(0.85))
                .child(SharedString::from(if reset.is_empty() {
                    format!("{pct:.0}%")
                } else {
                    format!("{pct:.0}% · {reset}")
                })),
        )
        .into_any_element()
}

/// Lo que se ve de una ficha del personalizador.
fn chip_face(row: &Row, shown: bool, width: f32, palette: &crate::Palette) -> gpui::Div {
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
                .text_size(px(12.))
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
    cx: &mut Context<Pill>,
) -> AnyElement {
    let (shown, out) = arranged(snap, prefs);
    let drag = view.drag.filter(|d| d.moved);
    let lane = |hidden: bool, rows: Vec<Row>, cx: &mut Context<Pill>| {
        let title = if hidden { "Fuera de la vista" } else { "A la vista" };
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
                    .text_size(px(10.5))
                    .text_color(palette.muted)
                    .child(title)
                    .child(div().text_color(palette.text.opacity(0.4)).child(SharedString::from(count.to_string()))),
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
                            .rounded(px(10.))
                            .border_1()
                            .border_color(palette.text.opacity(0.25))
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
                                .child(chip_face(&row, !hidden, chip_w, palette))
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
                                .text_size(px(11.))
                                .text_color(palette.text.opacity(0.35))
                                .child("Suelta aquí lo que no quieras ver"),
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
                    .text_size(px(10.5))
                    .text_color(gpui::Hsla::from(gpui::rgb(WARN)))
                    .child("Tiene que quedar al menos uno a la vista"),
            )
        })
        .into_any_element()
}
