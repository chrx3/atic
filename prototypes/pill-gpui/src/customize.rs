//! El editor de los niveles de la pill, como cara del notch.
//!
//! Tres filas —a la vista, detrás de «Más» y fuera— con las mismas fichas que
//! la tira. Se arrastran dentro de una fila o entre filas, y cada soltar se
//! guarda en el acto: no hay «Guardar», la tira queda como se la dejó. Con
//! teclado, flechas: ←/→ dentro de la fila, ↑/↓ entre filas.
//!
//! La ficha arrastrada sale de su fila (queda un fantasma bajo el cursor) y la
//! fila destino le abre un hueco. El puntero se captura en la raíz y no en la
//! ficha: la ficha se desmonta al salir de su fila y se llevaría la captura.
//!
//! El hueco se calcula con la grilla y no midiendo las fichas
//! (`slot_index`): al abrirlo, las fichas se corren, y medir sus posiciones
//! movería el hueco otra vez —el índice oscilaría bajo el cursor—. Las fichas
//! son todas del mismo tamaño, así que la grilla basta.

use std::cell::RefCell;
use std::time::Instant;

use gpui::{
    actions, div, prelude::*, px, rgb, svg, AnyElement, App, ClickEvent, Context, ElementId,
    EventEmitter, FocusHandle, Focusable, KeyBinding, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, Pixels, Window,
};

use crate::clipboard::{BAND_H, MARK_GAP};
use crate::CUSTOMIZE_INSET;
use crate::hover::HoverExt;
use crate::pill_tools::{self, Bucket, Layout, BUCKETS};

/// Lado de una ficha y hueco entre fichas (px). `layout` usa estos dos.
const CHIP: f32 = 34.0;
const GAP: f32 = 6.0;
const ROW_PAD: f32 = 6.0;
const LABEL_H: f32 = 18.0;
/// Lo que mide una fila: rótulo, ficha y los bordes de la fila.
const ROW_H: f32 = LABEL_H + CHIP + ROW_PAD * 2.0;
const HEAD_H: f32 = 30.0;
/// Se separa 4 px antes de que el movimiento cuente como arrastre.
const DRAG_THRESHOLD: f32 = 4.0;
/// Cuánto dura el rebote de la ficha que no pudo ir donde se la soltó.
const REFUSE_MS: f32 = 360.0;

actions!(customize_panel, [Dismiss]);

const KEY_CONTEXT: &str = "CustomizePanel";

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([KeyBinding::new("escape", Dismiss, Some(KEY_CONTEXT))]);
}

pub enum CustomizeEvent {
    Close,
    /// La disposición quedó como se la dejó: la pill la guarda y la tira ya la
    /// muestra así.
    Placed(Layout),
}

/// Dónde cae una ficha arrastrada.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct DropSlot {
    pub(crate) bucket: Bucket,
    pub(crate) index: usize,
}

/// Las medidas de una fila: toda la sección (rótulo + fichas) y la caja de las
/// fichas, donde empieza la grilla.
#[derive(Clone, Copy)]
pub(crate) struct DropRow {
    bucket: Bucket,
    zone: gpui::Bounds<Pixels>,
    grid: gpui::Bounds<Pixels>,
    /// Cuántas fichas hay en la fila, sin la que se arrastra.
    count: usize,
}

impl DropRow {
    /// Qué tan lejos está el punto de esta fila, en vertical (0 si está encima).
    fn distance(&self, y: f32) -> f32 {
        let top = f32::from(self.zone.origin.y);
        let bottom = top + f32::from(self.zone.size.height);
        if y < top {
            top - y
        } else if y > bottom {
            y - bottom
        } else {
            0.0
        }
    }
}

/// Posición de inserción dentro de una fila, por grilla.
pub(crate) fn slot_index(grid: gpui::Bounds<Pixels>, count: usize, x: f32, y: f32) -> usize {
    let pitch = CHIP + GAP;
    let per_line = per_line(f32::from(grid.size.width));
    let line = (((y - f32::from(grid.origin.y)) / pitch).floor().max(0.0)) as usize;
    // Cuántas fichas de esa línea tienen el centro antes del cursor.
    // Pasado el centro de la última ficha de una línea, el hueco va al final
    // de esa línea solo si es la última; si no, sería el principio de la
    // siguiente y aparecería una línea más abajo que el cursor.
    let last_line = line + 1 >= count.div_ceil(per_line).max(1);
    let max_col = if last_line { per_line } else { per_line - 1 };
    let col = (((x - f32::from(grid.origin.x) - CHIP / 2.0) / pitch).floor() as isize + 1)
        .clamp(0, max_col as isize) as usize;
    (line * per_line + col).min(count)
}

/// Cuántas fichas caben en una línea de `width`.
fn per_line(width: f32) -> usize {
    (((width + GAP) / (CHIP + GAP)).floor() as usize).max(1)
}

/// Alto de la grilla de una fila: al menos una línea, aunque esté vacía.
fn grid_height(count: usize, per_line: usize) -> f32 {
    let lines = count.div_ceil(per_line.max(1)).max(1) as f32;
    lines * CHIP + (lines - 1.0) * GAP
}

/// Fila bajo el cursor (o la más cercana en vertical) y su índice.
pub(crate) fn drop_slot(rows: &[DropRow], x: f32, y: f32) -> Option<DropSlot> {
    let best = rows
        .iter()
        .min_by(|a, b| a.distance(y).total_cmp(&b.distance(y)))?;
    Some(DropSlot {
        bucket: best.bucket,
        index: slot_index(best.grid, best.count, x, y),
    })
}

pub struct CustomizePanel {
    focus: FocusHandle,
    layout: Layout,
    /// Los límites de cada fila del último cuadro, para el clic y el arrastre.
    rows: RefCell<Vec<DropRow>>,
    /// La ficha que se está arrastrando.
    drag: Option<Drag>,
    /// Ficha que no pudo ir donde se la soltó: rebota.
    refused: Option<(usize, Instant)>,
    /// La ficha desde la que se abrió el editor (presión larga), para que el
    /// ojo la encuentre.
    focus_chip: Option<usize>,
    /// La ficha que se está arrastrando (la pinta el cuadro) y dónde está el
    /// cursor. El manejador de movimiento los deja; el cuadro solo los pinta.
    ghost: Option<usize>,
    ghost_at: (f32, f32),
    pub max_height: Option<f32>,
    pub mark_gap: bool,
    colors: Colors,
}

/// Lo que se sabe de la ficha apretada.
struct Drag {
    tool: usize,
    from: Bucket,
    /// Dónde se apretó, en coordenadas de la raíz.
    start: gpui::Point<Pixels>,
    moving: bool,
    /// El hueco que la fila destino le abre.
    slot: Option<DropSlot>,
}

#[derive(Clone)]
struct Colors {
    text: gpui::Hsla,
    muted: gpui::Hsla,
    faint: gpui::Hsla,
    warn: gpui::Hsla,
}

impl EventEmitter<CustomizeEvent> for CustomizePanel {}

impl Focusable for CustomizePanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl CustomizePanel {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus: cx.focus_handle(),
            layout: pill_tools::load(),
            rows: RefCell::new(Vec::new()),
            drag: None,
            refused: None,
            focus_chip: None,
            ghost: None,
            ghost_at: (0., 0.),
            max_height: None,
            mark_gap: false,
            colors: Colors {
                text: rgb(0xf0f0ea).into(),
                muted: rgb(0x9a9a90).into(),
                faint: rgb(0x6e6e66).into(),
                warn: rgb(0xe8b04b).into(),
            },
        }
    }

    /// Al abrir: vuelve a leer lo guardado, para que el editor muestre lo que
    /// está en la tira ahora.
    pub fn reset(&mut self, cx: &mut Context<Self>) {
        self.layout = pill_tools::load();
        self.drag = None;
        self.refused = None;
        cx.notify();
    }

    /// Deja la disposición como quedó. La guarda la pill: el editor solo dice
    /// qué se pidió.
    pub fn set(&mut self, layout: Layout) {
        self.layout = layout;
        self.drag = None;
    }

    /// La ficha desde la que se abrió el editor, para destacarla.
    pub fn focus_tool(&mut self, tool: Option<usize>) {
        self.focus_chip = tool;
    }

    pub fn desired_height(&self) -> f32 {
        let inner = crate::CUSTOMIZE_W - CUSTOMIZE_INSET * 2.0;
        let rows = self.row_heights(inner);
        let note = if self.ring_locked() { 18.0 } else { 0.0 };
        (BAND_H + HEAD_H + rows + note + 10.0).min(self.max_height.unwrap_or(f32::MAX))
    }

    /// Cuántas fichas caben en una línea de la fila.
    fn per_line(&self, inner: f32) -> usize {
        per_line(inner - ROW_PAD * 2.0)
    }

    /// El anillo no puede quedar vacío: sacar la última rebota.
    fn ring_locked(&self) -> bool {
        self.layout.ring.len() <= 1
    }

    /// Sacar la última del anillo no deja la pill sin nada a la vista.
    fn can_go(&self, from: Bucket, to: Bucket) -> bool {
        !(from == Bucket::Ring && to != Bucket::Ring && self.ring_locked())
    }

    /// Cuánto mide cada fila: el rótulo y las grillas de fichas que quepan en
    /// `w`. Se calcula una vez por cuadro y es lo que usan el dibujo y el clic.
    fn row_heights(&self, w: f32) -> f32 {
        let per_line = self.per_line(w);
        let mut total = 0.0;
        for bucket in BUCKETS {
            // La ficha arrastrada no ocupa sitio: la fila destino le abre el hueco.
            let count = match &self.drag {
                Some(drag) if drag.moving => {
                    self.layout.list(bucket).len().saturating_sub(usize::from(bucket == drag.from))
                }
                _ => self.layout.list(bucket).len(),
            };
            total += LABEL_H + grid_height(count, per_line) + ROW_PAD * 2.0;
        }
        total
    }

    /// Lo que pinta una fila: sin la arrastrada y con el hueco donde caería.
    fn cells(&self, bucket: Bucket) -> Vec<Option<usize>> {
        let tools = self.layout.list(bucket);
        let Some(drag) = self.drag.as_ref().filter(|drag| drag.moving) else {
            return tools.iter().map(|&tool| Some(tool)).collect();
        };
        let mut cells: Vec<Option<usize>> = tools
            .iter()
            .filter(|&&tool| tool != drag.tool)
            .map(|&tool| Some(tool))
            .collect();
        if drag.slot.is_some_and(|slot| slot.bucket == bucket) {
            let at = drag.slot.map(|slot| slot.index).unwrap_or_default();
            cells.insert(at.min(cells.len()), None);
        }
        cells
    }

    /// Fuera no tiene orden propio: cae donde lo pone el catálogo.
    fn drop_index(&self, drag: &Drag, slot: DropSlot) -> usize {
        if slot.bucket == Bucket::Hidden {
            pill_tools::hidden_index(&self.layout, drag.tool)
        } else {
            slot.index
        }
    }

    fn on_chip_down(&mut self, event: &MouseDownEvent, tool: usize, from: Bucket, cx: &mut Context<Self>) {
        if self.drag.is_some() {
            return;
        }
        self.refused = None;
        self.ghost = None;
        self.drag = Some(Drag {
            tool,
            from,
            start: event.position,
            moving: false,
            slot: None,
        });
        cx.notify();
    }

    fn on_move(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        let Some(drag) = self.drag.as_mut() else {
            return;
        };
        let delta = event.position - drag.start;
        let moved = f32::from(delta.x).hypot(f32::from(delta.y));
        if !drag.moving && moved < DRAG_THRESHOLD {
            return;
        }
        drag.moving = true;
        let (tool, from) = (drag.tool, drag.from);
        let (x, y) = (f32::from(event.position.x), f32::from(event.position.y));
        self.ghost = Some(tool);
        self.ghost_at = (x, y);
        let rows = self.rows.borrow().clone();
        let slot = drop_slot(&rows, x, y).filter(|slot| self.can_go(from, slot.bucket));
        if let Some(drag) = self.drag.as_mut() {
            drag.slot = slot;
        }
        cx.notify();
    }

    fn on_up(&mut self, _: &MouseUpEvent, cx: &mut Context<Self>) {
        let Some(drag) = self.drag.take() else {
            return;
        };
        if !drag.moving {
            return;
        }
        let Some(slot) = drag.slot else {
            self.refused = Some((drag.tool, Instant::now()));
            cx.notify();
            return;
        };
        let Some(next) = pill_tools::place(&self.layout, drag.tool, slot.bucket, Some(self.drop_index(&drag, slot)))
        else {
            self.refused = Some((drag.tool, Instant::now()));
            cx.notify();
            return;
        };
        let (ring, more) = next;
        self.layout = pill_tools::layout(&ring, &more);
        cx.emit(CustomizeEvent::Placed(self.layout.clone()));
        cx.notify();
    }

    fn reset_layout(&mut self, cx: &mut Context<Self>) {
        self.layout = pill_tools::layout(&[], &[]);
        cx.emit(CustomizeEvent::Placed(self.layout.clone()));
        cx.notify();
    }

    /// Una ficha: el ícono en un círculo, o el hueco de la fila destino.
    fn chip(&self, tool: Option<usize>, bucket: Bucket, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let c = self.colors.clone();
        let Some(tool) = tool else {
            return div()
                .size(px(CHIP))
                .flex_none()
                .rounded_full()
                .border_1()
                .border_color(hsla(c.text, 0.32))
                .into_any_element();
        };
        let _ = index;
        let refused = self
            .refused
            .is_some_and(|(at, since)| at == tool && since.elapsed().as_secs_f32() * 1000.0 < REFUSE_MS);
        let found = self.focus_chip == Some(tool);
        // Fuera va más apagada: sin fondo, solo el contorno.
        let (rest, over) = if bucket_is_dark(bucket) {
            (hsla(c.text, 0.0), hsla(c.text, 0.08))
        } else {
            (hsla(c.text, 0.08), hsla(c.text, 0.16))
        };
        div()
            .id(ElementId::NamedInteger("customize-chip".into(), tool as u64))
            .size(px(CHIP))
            .flex_none()
            .rounded_full()
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |panel, event: &MouseDownEvent, _, cx| {
                    panel.on_chip_down(event, tool, bucket, cx)
                }),
            )
            .when(bucket == Bucket::Hidden, |el| {
                el.border_1().border_color(hsla(c.text, 0.14))
            })
            // La que abrió el editor: un anillo para que el ojo la encuentre.
            .when(found, |el| el.border_2().border_color(hsla(c.text, 0.55)))
            .when(refused, |el| el.opacity(0.5))
            .child(
                svg()
                    .path(pill_tools::icon(tool))
                    .size(px(18.))
                    .text_color(if bucket == Bucket::Hidden { c.faint } else { c.muted }),
            )
            .hover_bg(ElementId::NamedInteger("customize-chip-fx".into(), tool as u64), rest, over)
            .into_any_element()
    }

    fn body(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let c = self.colors.clone();
        let inner = crate::CUSTOMIZE_W - CUSTOMIZE_INSET * 2.0;
        let per_line = self.per_line(inner);
        // Las medidas de las filas se dejan listas antes de pintar: el arrastre
        // y el clic las leen, y tienen que ser las mismas que se ven.
        let mut rows = Vec::with_capacity(BUCKETS.len());
        let mut y = BAND_H + HEAD_H;
        for bucket in BUCKETS {
            let cells = self.cells(bucket);
            let grid_h = grid_height(cells.len(), per_line);
            let h = LABEL_H + grid_h + ROW_PAD * 2.0;
            let x = CUSTOMIZE_INSET;
            rows.push(DropRow {
                bucket,
                zone: gpui::Bounds::new(
                    gpui::point(px(x), px(y)),
                    gpui::size(px(crate::CUSTOMIZE_W - CUSTOMIZE_INSET * 2.0), px(h)),
                ),
                grid: gpui::Bounds::new(
                    gpui::point(px(x + ROW_PAD), px(y + LABEL_H + ROW_PAD)),
                    gpui::size(px(inner - ROW_PAD * 2.0), px(grid_h)),
                ),
                count: cells.len(),
            });
            y += h;
        }
        *self.rows.borrow_mut() = rows;

        let head = div()
            .h(px(HEAD_H))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(8.))
            .pr(px(8.))
            .child(div().flex_1().child(div().text_size(px(13.)).text_color(c.text).child("Personalizar la pill")))
            .child(
                div()
                    .id("customize-appearance")
                    .text_size(px(11.))
                    .text_color(c.faint)
                    .cursor_pointer()
                    .rounded_full()
                    .px(px(8.))
                    .py(px(3.))
                    .on_click(cx.listener(|_, _: &ClickEvent, _, cx| cx.defer(crate::settings::open)))
                    .child("Ajustes")
                    .hover_bg("customize-appearance-fx", hsla(c.text, 0.10), hsla(c.text, 0.16)),
            )
            .child(
                div()
                    .id("customize-reset")
                    .text_size(px(11.))
                    .text_color(c.faint)
                    .cursor_pointer()
                    .rounded_full()
                    .px(px(8.))
                    .py(px(3.))
                    .on_click(cx.listener(|panel, _: &ClickEvent, _, cx| panel.reset_layout(cx)))
                    .child("Restablecer")
                    .hover_bg("customize-reset-fx", hsla(c.text, 0.10), hsla(c.text, 0.16)),
            )
            .child(
                div()
                    .id("customize-done")
                    .text_size(px(11.))
                    .text_color(c.text)
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .cursor_pointer()
                    .rounded_full()
                    .bg(hsla(c.text, 0.12))
                    .px(px(12.))
                    .py(px(4.))
                    .on_click(cx.listener(|_, _: &ClickEvent, _, cx| {
                        cx.emit(CustomizeEvent::Close)
                    }))
                    .child("Listo")
                    .hover_bg("customize-done-fx", hsla(c.text, 0.18), hsla(c.text, 0.24)),
            );

        let mut list = div().flex().flex_col().px(px(CUSTOMIZE_INSET));
        for bucket in BUCKETS {
            let cells = self.cells(bucket);
            let count = self.layout.list(bucket).len();
            let target = self.drag.as_ref().is_some_and(|drag| {
                drag.moving && drag.slot.is_some_and(|slot| slot.bucket == bucket)
            });
            let refused_row = self.drag.as_ref().is_some_and(|drag| {
                drag.moving
                    && drag.slot.is_none()
                    && drag.from == Bucket::Ring
                    && self.ring_locked()
                    && bucket != Bucket::Ring
            });
            let grid = div()
                .flex()
                .flex_wrap()
                .gap(px(GAP))
                .min_h(px(CHIP))
                .children(
                    cells
                        .iter()
                        .enumerate()
                        .map(|(index, tool)| self.chip(*tool, bucket, index, cx)),
                )
                .when(cells.is_empty(), |el| {
                    el.child(
                        div()
                            .h(px(CHIP))
                            .flex_1()
                            .rounded_full()
                            .border_1()
                            .border_color(hsla(c.text, 0.12))
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(11.))
                            .text_color(c.faint)
                            .child("Suelta aquí"),
                    )
                });
            list = list.child(
                div()
                    .flex_none()
                    .rounded(px(8.))
                    .px(px(ROW_PAD))
                    .py(px(ROW_PAD))
                    .when(target, |el| el.bg(hsla(c.text, 0.06)))
                    .when(refused_row, |el| el.opacity(0.45))
                    .child(
                        div()
                            .h(px(LABEL_H))
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(c.faint)
                                    .child(pill_tools::bucket_title(bucket)),
                            )
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(c.faint)
                                    .opacity(0.7)
                                    .child(format!("{count}")),
                            ),
                    )
                    .child(grid),
            );
        }
        if self.ring_locked() {
            list = list.child(
                div()
                    .text_size(px(11.))
                    .text_color(c.warn)
                    .child("Tiene que quedar al menos una a la vista"),
            );
        }

        // El fantasma: la ficha que se arrastra, bajo el cursor.
        let ghost = self.ghost.and_then(|tool| {
            self.drag
                .as_ref()
                .filter(|drag| drag.moving)
                .map(|_| tool)
        });
        let ghost = ghost.map(|tool| {
            div()
                .absolute()
                .left(px(self.ghost_at.0 - CUSTOMIZE_INSET - CHIP / 2.0))
                .top(px(self.ghost_at.1 - CHIP / 2.0))
                .size(px(CHIP))
                .rounded_full()
                .flex()
                .items_center()
                .justify_center()
                .bg(hsla(c.text, 0.22))
                .child(svg().path(pill_tools::icon(tool)).size(px(18.)).text_color(c.text))
        });

        div()
            .relative()
            .flex()
            .flex_col()
            .w_full()
            .on_mouse_move(cx.listener(|panel, event: &MouseMoveEvent, _, cx| {
                panel.on_move(event, cx)
            }))
            .on_mouse_up(MouseButton::Left, cx.listener(|panel, event: &MouseUpEvent, _, cx| {
                panel.on_up(event, cx)
            }))
            .on_mouse_up_out(MouseButton::Left, cx.listener(|panel, event: &MouseUpEvent, _, cx| {
                panel.on_up(event, cx)
            }))
            .child(div().h(px(BAND_H)).flex_none().child(
                div()
                    .id("customize-mark")
                    .w(px(MARK_GAP))
                    .h_full()
                    .flex_none()
                    .cursor_pointer()
                    .on_click(cx.listener(|_, _: &ClickEvent, _, cx| {
                        cx.emit(CustomizeEvent::Close)
                    })),
            ))
            .child(head)
            .child(list)
            .children(ghost)
            .into_any_element()
    }
}

fn bucket_is_dark(bucket: Bucket) -> bool {
    matches!(bucket, Bucket::Hidden)
}

fn hsla(color: gpui::Hsla, alpha: f32) -> gpui::Hsla {
    color.opacity(alpha)
}

impl Render for CustomizePanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus)
            .on_action(cx.listener(|_, _: &Dismiss, _, cx| cx.emit(CustomizeEvent::Close)))
            .size_full()
            .font_family("Segoe UI")
            .child(self.body(cx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::point;
    use gpui::size;

    /// Una grilla donde caben `n` fichas por línea, con sus huecos.
    fn grid(n: f32) -> gpui::Bounds<Pixels> {
        let w = n * CHIP + (n - 1.0) * GAP;
        gpui::Bounds::new(point(px(0.), px(0.)), size(px(w), px(CHIP)))
    }

    #[test]
    fn la_grilla_de_a_uno_solo_es_el_principio_de_la_fila() {
        assert_eq!(slot_index(grid(1.), 4, 0., 0.), 0);
        assert_eq!(slot_index(grid(1.), 4, 33., 0.), 0);
    }

    #[test]
    fn a_la_derecha_del_centro_de_una_ficha_es_la_siguiente() {
        let pitch = CHIP + GAP;
        let center = CHIP / 2.0;
        assert_eq!(slot_index(grid(4.), 4, center + 1., 0.), 1);
        assert_eq!(slot_index(grid(4.), 4, center + pitch + 1., 0.), 2);
    }

    #[test]
    fn nunca_mas_alla_del_final_de_la_fila() {
        assert_eq!(slot_index(grid(2.), 2, 34. * 6.0, 0.), 2);
        assert_eq!(slot_index(grid(2.), 0, 34. * 3.0, 0.), 0);
    }

    #[test]
    fn salta_de_linea_cuando_el_cursor_pasa_a_la_siguiente() {
        let pitch = CHIP + GAP;
        assert_eq!(slot_index(grid(2.), 12, 0., pitch + 5.), 2);
    }

    #[test]
    fn elige_la_fila_mas_cercana_en_vertical() {
        let row = |bucket, top| DropRow {
            bucket,
            zone: gpui::Bounds::new(point(px(0.), px(top)), size(px(100.), px(40.))),
            grid: gpui::Bounds::new(point(px(0.), px(top)), size(px(100.), px(34.))),
            count: 3,
        };
        let rows = [row(Bucket::Ring, 0.), row(Bucket::More, 40.), row(Bucket::Hidden, 80.)];
        assert_eq!(drop_slot(&rows, 10., 10.).unwrap().bucket, Bucket::Ring);
        assert_eq!(drop_slot(&rows, 10., 50.).unwrap().bucket, Bucket::More);
        // Entre dos filas gana la más cercana, no la de arriba.
        assert_eq!(drop_slot(&rows, 10., 78.).unwrap().bucket, Bucket::More);
        assert_eq!(drop_slot(&rows, 10., 82.).unwrap().bucket, Bucket::Hidden);
        // Lejos de todas, también.
        assert_eq!(drop_slot(&rows, 10., 400.).unwrap().bucket, Bucket::Hidden);
    }

    #[test]
    fn sin_filas_no_hay_hueco() {
        assert_eq!(drop_slot(&[], 0., 0.), None);
    }
}
