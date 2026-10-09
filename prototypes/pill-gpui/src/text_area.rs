//! Editor de texto multilínea con ajuste de línea, para el bloc. Hermano de
//! `text_input.rs` (mismo origen: el ejemplo `input.rs` de gpui 0.2), con las
//! acciones de ahí; solo añade Enter, ↑/↓ y desplazamiento vertical.

use std::ops::Range;

use gpui::{
    actions, div, fill, point, prelude::*, px, relative, size, App, Bounds, ClipboardItem, Context,
    CursorStyle, ElementId, ElementInputHandler, Entity, EntityInputHandler, EventEmitter,
    FocusHandle, Focusable, GlobalElementId, Hsla, KeyBinding, LayoutId, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, PaintQuad, Pixels, Point, ScrollWheelEvent,
    SharedString, Style, TextAlign, TextRun, UTF16Selection, UnderlineStyle, Window, WrappedLine,
};
use unicode_segmentation::UnicodeSegmentation;

use crate::text_input::{
    Backspace, Copy, Cut, Delete, End, Home, Left, Paste, Right, SelectAll, SelectLeft,
    SelectRight, Changed,
};

actions!(text_area, [Newline, Up, Down, SelectUp, SelectDown]);

pub const KEY_CONTEXT: &str = "TextArea";

pub fn bind_keys(cx: &mut App) {
    let context = Some(KEY_CONTEXT);
    cx.bind_keys([
        KeyBinding::new("backspace", Backspace, context),
        KeyBinding::new("delete", Delete, context),
        KeyBinding::new("left", Left, context),
        KeyBinding::new("right", Right, context),
        KeyBinding::new("up", Up, context),
        KeyBinding::new("down", Down, context),
        KeyBinding::new("shift-left", SelectLeft, context),
        KeyBinding::new("shift-right", SelectRight, context),
        KeyBinding::new("shift-up", SelectUp, context),
        KeyBinding::new("shift-down", SelectDown, context),
        KeyBinding::new("enter", Newline, context),
        KeyBinding::new("ctrl-a", SelectAll, context),
        KeyBinding::new("ctrl-v", Paste, context),
        KeyBinding::new("ctrl-c", Copy, context),
        KeyBinding::new("ctrl-x", Cut, context),
        KeyBinding::new("home", Home, context),
        KeyBinding::new("end", End, context),
    ]);
}

pub struct TextArea {
    focus_handle: FocusHandle,
    content: SharedString,
    placeholder: SharedString,
    text_color: Hsla,
    placeholder_color: Hsla,
    accent: Hsla,
    selected_range: Range<usize>,
    selection_reversed: bool,
    marked_range: Option<Range<usize>>,
    /// Una `WrappedLine` por línea lógica, con su byte inicial en `content`.
    layout: Vec<(usize, WrappedLine)>,
    bounds: Option<Bounds<Pixels>>,
    line_height: Pixels,
    scroll_y: Pixels,
    /// Mantener el cursor a la vista en el próximo cuadro (al teclear o mover).
    reveal_cursor: bool,
    /// Columna que ↑/↓ intentan conservar.
    goal_x: Option<Pixels>,
    is_selecting: bool,
    /// Filas que ocupa el texto ya distribuido, para que la caja crezca con él.
    rows: usize,
}

impl EventEmitter<Changed> for TextArea {}

impl TextArea {
    pub fn new(
        placeholder: impl Into<SharedString>,
        text_color: Hsla,
        placeholder_color: Hsla,
        accent: Hsla,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            content: SharedString::default(),
            placeholder: placeholder.into(),
            text_color,
            placeholder_color,
            accent,
            selected_range: 0..0,
            selection_reversed: false,
            marked_range: None,
            layout: Vec::new(),
            bounds: None,
            line_height: px(18.),
            scroll_y: px(0.),
            reveal_cursor: false,
            goal_x: None,
            is_selecting: false,
            rows: 1,
        }
    }

    pub fn set_placeholder(&mut self, placeholder: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.placeholder = placeholder.into();
        cx.notify();
    }

    pub fn placeholder(&self) -> &str {
        &self.placeholder
    }

    /// El alto del texto en el último cuadro dibujado, con un mínimo de filas.
    pub fn content_height(&self, min_rows: usize) -> Pixels {
        self.line_height * self.rows.max(min_rows) as f32
    }

    pub fn text(&self) -> &str {
        &self.content
    }

    /// Cambia el texto sin avisar (`Changed` es lo que escribe el usuario).
    pub fn set_text(&mut self, text: &str, cx: &mut Context<Self>) {
        self.content = text.to_string().into();
        let end = self.content.len();
        self.selected_range = end..end;
        self.selection_reversed = false;
        self.marked_range = None;
        self.scroll_y = px(0.);
        cx.notify();
    }

    fn cursor_offset(&self) -> usize {
        if self.selection_reversed {
            self.selected_range.start
        } else {
            self.selected_range.end
        }
    }

    fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.selected_range = offset..offset;
        self.selection_reversed = false;
        self.goal_x = None;
        self.reveal_cursor = true;
        cx.notify()
    }

    fn select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        if self.selection_reversed {
            self.selected_range.start = offset
        } else {
            self.selected_range.end = offset
        };
        if self.selected_range.end < self.selected_range.start {
            self.selection_reversed = !self.selection_reversed;
            self.selected_range = self.selected_range.end..self.selected_range.start;
        }
        self.reveal_cursor = true;
        cx.notify()
    }

    // --- Geometría ---------------------------------------------------------

    /// Posición del byte `offset`, relativa al origen del texto.
    fn point_for(&self, offset: usize) -> Point<Pixels> {
        let mut y = px(0.);
        for (start, line) in &self.layout {
            let end = start + line.len();
            if offset >= *start && offset <= end {
                let local = line
                    .position_for_index(offset - start, self.line_height)
                    .unwrap_or_default();
                return point(local.x, y + local.y);
            }
            y += line.size(self.line_height).height;
        }
        point(px(0.), y)
    }

    /// Byte más cercano a un punto relativo al origen del texto.
    fn offset_for(&self, p: Point<Pixels>) -> usize {
        let mut y = px(0.);
        for (start, line) in &self.layout {
            let height = line.size(self.line_height).height;
            if p.y < y + height {
                let local = point(p.x, p.y - y);
                let index = match line.closest_index_for_position(local, self.line_height) {
                    Ok(index) | Err(index) => index,
                };
                return start + index.min(line.len());
            }
            y += height;
        }
        self.content.len()
    }

    fn text_height(&self) -> Pixels {
        self.layout
            .iter()
            .map(|(_, line)| line.size(self.line_height).height)
            .fold(px(0.), |a, b| a + b)
    }

    fn local(&self, position: Point<Pixels>) -> Point<Pixels> {
        let origin = self.bounds.map(|b| b.origin).unwrap_or_default();
        point(position.x - origin.x, position.y - origin.y + self.scroll_y)
    }

    // --- Acciones ----------------------------------------------------------

    fn line_bounds(&self, offset: usize) -> Range<usize> {
        let start = self.content[..offset].rfind('\n').map_or(0, |i| i + 1);
        let end = self.content[offset..]
            .find('\n')
            .map_or(self.content.len(), |i| offset + i);
        start..end
    }

    fn left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(self.previous_boundary(self.cursor_offset()), cx);
        } else {
            self.move_to(self.selected_range.start, cx)
        }
    }

    fn right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(self.next_boundary(self.selected_range.end), cx);
        } else {
            self.move_to(self.selected_range.end, cx)
        }
    }

    fn vertical(&self, direction: i32) -> usize {
        let cursor = self.cursor_offset();
        let here = self.point_for(cursor);
        let x = self.goal_x.unwrap_or(here.x);
        // El centro de la línea visual de destino.
        let y = here.y + self.line_height * direction as f32 + self.line_height * 0.5;
        if y < px(0.) {
            return 0;
        }
        if y > self.text_height() {
            return self.content.len();
        }
        self.offset_for(point(x, y))
    }

    fn up(&mut self, _: &Up, _: &mut Window, cx: &mut Context<Self>) {
        let goal = self.goal_x.unwrap_or(self.point_for(self.cursor_offset()).x);
        let target = self.vertical(-1);
        self.move_to(target, cx);
        self.goal_x = Some(goal);
    }

    fn down(&mut self, _: &Down, _: &mut Window, cx: &mut Context<Self>) {
        let goal = self.goal_x.unwrap_or(self.point_for(self.cursor_offset()).x);
        let target = self.vertical(1);
        self.move_to(target, cx);
        self.goal_x = Some(goal);
    }

    fn select_up(&mut self, _: &SelectUp, _: &mut Window, cx: &mut Context<Self>) {
        let target = self.vertical(-1);
        self.select_to(target, cx);
    }

    fn select_down(&mut self, _: &SelectDown, _: &mut Window, cx: &mut Context<Self>) {
        let target = self.vertical(1);
        self.select_to(target, cx);
    }

    fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.previous_boundary(self.cursor_offset()), cx);
    }

    fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.next_boundary(self.cursor_offset()), cx);
    }

    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
        self.select_to(self.content.len(), cx)
    }

    fn home(&mut self, _: &Home, _: &mut Window, cx: &mut Context<Self>) {
        let start = self.line_bounds(self.cursor_offset()).start;
        self.move_to(start, cx);
    }

    fn end(&mut self, _: &End, _: &mut Window, cx: &mut Context<Self>) {
        let end = self.line_bounds(self.cursor_offset()).end;
        self.move_to(end, cx);
    }

    fn newline(&mut self, _: &Newline, window: &mut Window, cx: &mut Context<Self>) {
        self.replace_text_in_range(None, "\n", window, cx)
    }

    fn backspace(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.select_to(self.previous_boundary(self.cursor_offset()), cx)
        }
        self.replace_text_in_range(None, "", window, cx)
    }

    fn delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.select_to(self.next_boundary(self.cursor_offset()), cx)
        }
        self.replace_text_in_range(None, "", window, cx)
    }

    fn on_mouse_down(&mut self, event: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.is_selecting = true;
        let offset = self.offset_for(self.local(event.position));
        if event.modifiers.shift {
            self.select_to(offset, cx);
        } else {
            self.move_to(offset, cx)
        }
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.is_selecting = false;
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.is_selecting {
            let offset = self.offset_for(self.local(event.position));
            self.select_to(offset, cx);
        }
    }

    fn on_scroll(&mut self, event: &ScrollWheelEvent, _: &mut Window, cx: &mut Context<Self>) {
        let delta = event.delta.pixel_delta(self.line_height).y;
        self.scroll_y -= delta;
        self.clamp_scroll();
        cx.notify();
    }

    fn clamp_scroll(&mut self) {
        let view = self.bounds.map_or(px(0.), |b| b.size.height);
        let max = (self.text_height() - view).max(px(0.));
        self.scroll_y = self.scroll_y.clamp(px(0.), max);
    }

    fn paste(&mut self, _: &Paste, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            self.replace_text_in_range(None, &text.replace("\r\n", "\n"), window, cx);
        }
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected_range.clone()].to_string(),
            ));
        }
    }

    fn cut(&mut self, _: &Cut, window: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected_range.clone()].to_string(),
            ));
            self.replace_text_in_range(None, "", window, cx)
        }
    }

    fn offset_from_utf16(&self, offset: usize) -> usize {
        let mut utf8_offset = 0;
        let mut utf16_count = 0;
        for ch in self.content.chars() {
            if utf16_count >= offset {
                break;
            }
            utf16_count += ch.len_utf16();
            utf8_offset += ch.len_utf8();
        }
        utf8_offset
    }

    fn offset_to_utf16(&self, offset: usize) -> usize {
        let mut utf16_offset = 0;
        let mut utf8_count = 0;
        for ch in self.content.chars() {
            if utf8_count >= offset {
                break;
            }
            utf8_count += ch.len_utf8();
            utf16_offset += ch.len_utf16();
        }
        utf16_offset
    }

    fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
    }

    fn range_from_utf16(&self, range_utf16: &Range<usize>) -> Range<usize> {
        self.offset_from_utf16(range_utf16.start)..self.offset_from_utf16(range_utf16.end)
    }

    fn previous_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .rev()
            .find_map(|(idx, _)| (idx < offset).then_some(idx))
            .unwrap_or(0)
    }

    fn next_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .find_map(|(idx, _)| (idx > offset).then_some(idx))
            .unwrap_or(self.content.len())
    }
}

impl EntityInputHandler for TextArea {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.range_from_utf16(&range_utf16);
        actual_range.replace(self.range_to_utf16(&range));
        Some(self.content[range].to_string())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.range_to_utf16(&self.selected_range),
            reversed: self.selection_reversed,
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked_range
            .as_ref()
            .map(|range| self.range_to_utf16(range))
    }

    fn unmark_text(&mut self, _: &mut Window, _: &mut Context<Self>) {
        self.marked_range = None;
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|range_utf16| self.range_from_utf16(range_utf16))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());

        self.content =
            (self.content[0..range.start].to_owned() + new_text + &self.content[range.end..])
                .into();
        self.selected_range = range.start + new_text.len()..range.start + new_text.len();
        self.selection_reversed = false;
        self.marked_range.take();
        self.goal_x = None;
        self.reveal_cursor = true;
        cx.emit(Changed);
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|range_utf16| self.range_from_utf16(range_utf16))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());

        self.content =
            (self.content[0..range.start].to_owned() + new_text + &self.content[range.end..])
                .into();
        self.marked_range = if new_text.is_empty() {
            None
        } else {
            Some(range.start..range.start + new_text.len())
        };
        self.selected_range = new_selected_range_utf16
            .as_ref()
            .map(|range_utf16| self.range_from_utf16(range_utf16))
            .map(|new_range| new_range.start + range.start..new_range.end + range.end)
            .unwrap_or_else(|| range.start + new_text.len()..range.start + new_text.len());
        self.reveal_cursor = true;
        cx.emit(Changed);
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let range = self.range_from_utf16(&range_utf16);
        let start = self.point_for(range.start);
        let end = self.point_for(range.end);
        Some(Bounds::from_corners(
            point(
                bounds.left() + start.x,
                bounds.top() + start.y - self.scroll_y,
            ),
            point(
                bounds.left() + end.x.max(start.x + px(1.)),
                bounds.top() + start.y - self.scroll_y + self.line_height,
            ),
        ))
    }

    fn character_index_for_point(
        &mut self,
        position: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        let offset = self.offset_for(self.local(position));
        Some(self.offset_to_utf16(offset))
    }
}

struct AreaElement {
    input: Entity<TextArea>,
}

struct PrepaintState {
    lines: Vec<(usize, WrappedLine)>,
    cursor: Option<PaintQuad>,
    selection: Vec<PaintQuad>,
}

impl IntoElement for AreaElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl gpui::Element for AreaElement {
    type RequestLayoutState = ();
    type PrepaintState = PrepaintState;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = relative(1.).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let line_height = window.line_height();
        let style = window.text_style();
        let font_size = style.font_size.to_pixels(window.rem_size());
        let (content, placeholder, text_color, placeholder_color, marked, accent) = {
            let input = self.input.read(cx);
            (
                input.content.clone(),
                input.placeholder.clone(),
                input.text_color,
                input.placeholder_color,
                input.marked_range.clone(),
                input.accent,
            )
        };

        let (display, color) = if content.is_empty() {
            (placeholder, placeholder_color)
        } else {
            (content.clone(), text_color)
        };
        let run = TextRun {
            len: display.len(),
            font: style.font(),
            color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let runs: Vec<TextRun> = match marked.filter(|_| !content.is_empty()) {
            Some(marked) => vec![
                TextRun {
                    len: marked.start,
                    ..run.clone()
                },
                TextRun {
                    len: marked.end - marked.start,
                    underline: Some(UnderlineStyle {
                        color: Some(run.color),
                        thickness: px(1.0),
                        wavy: false,
                    }),
                    ..run.clone()
                },
                TextRun {
                    len: display.len() - marked.end,
                    ..run
                },
            ]
            .into_iter()
            .filter(|run| run.len > 0)
            .collect(),
            None => vec![run],
        };

        let shaped = window
            .text_system()
            .shape_text(display.clone(), font_size, &runs, Some(bounds.size.width), None)
            .unwrap_or_default();
        // Cada línea lógica empieza tras el `\n` de la anterior.
        let mut start = 0;
        let lines: Vec<(usize, WrappedLine)> = shaped
            .into_iter()
            .map(|line| {
                let at = start;
                start += line.len() + 1;
                (at, line)
            })
            .collect();

        // Se guarda el diseño para ratón y teclado, y se ajusta el scroll.
        let rows = lines.iter().map(|(_, line)| line.wrap_boundaries().len() + 1).sum::<usize>().max(1);
        let (selected, cursor_offset, scroll_y, rows_changed) = self.input.update(cx, |input, _| {
            let rows_changed = input.rows != rows;
            input.rows = rows;
            if !content.is_empty() {
                input.layout = lines.clone();
            } else {
                input.layout = Vec::new();
            }
            input.bounds = Some(bounds);
            input.line_height = line_height;
            if input.reveal_cursor {
                input.reveal_cursor = false;
                let y = input.point_for(input.cursor_offset()).y;
                if y < input.scroll_y {
                    input.scroll_y = y;
                } else if y + line_height > input.scroll_y + bounds.size.height {
                    input.scroll_y = y + line_height - bounds.size.height;
                }
            }
            input.clamp_scroll();
            (
                input.selected_range.clone(),
                input.cursor_offset(),
                input.scroll_y,
                rows_changed,
            )
        });
        // La caja que la contiene toma su alto del cuadro anterior.
        if rows_changed {
            window.refresh();
        }

        let origin = point(bounds.left(), bounds.top() - scroll_y);
        let mut selection = Vec::new();
        let mut cursor = None;
        if !content.is_empty() {
            let input = self.input.read(cx);
            if selected.is_empty() {
                let p = input.point_for(cursor_offset);
                cursor = Some(fill(
                    Bounds::new(
                        point(origin.x + p.x, origin.y + p.y + px(1.)),
                        size(px(1.5), line_height - px(2.)),
                    ),
                    text_color,
                ));
            } else {
                // Un rectángulo por línea visual tocada por la selección.
                let first = input.point_for(selected.start);
                let last = input.point_for(selected.end);
                let rows = ((last.y - first.y) / line_height).round() as i32;
                for row in 0..=rows {
                    let y = first.y + line_height * row as f32;
                    let left = if row == 0 { first.x } else { px(0.) };
                    let right = if row == rows {
                        last.x
                    } else {
                        bounds.size.width
                    };
                    selection.push(fill(
                        Bounds::from_corners(
                            point(origin.x + left, origin.y + y),
                            point(origin.x + right.max(left + px(4.)), origin.y + y + line_height),
                        ),
                        accent.opacity(0.35),
                    ));
                }
            }
        } else {
            cursor = Some(fill(
                Bounds::new(
                    point(origin.x, origin.y + px(1.)),
                    size(px(1.5), line_height - px(2.)),
                ),
                text_color,
            ));
        }

        PrepaintState {
            lines,
            cursor,
            selection,
        }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus_handle = self.input.read(cx).focus_handle.clone();
        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(bounds, self.input.clone()),
            cx,
        );
        let line_height = window.line_height();
        let scroll_y = self.input.read(cx).scroll_y;
        window.with_content_mask(Some(gpui::ContentMask { bounds }), |window| {
            for quad in prepaint.selection.drain(..) {
                window.paint_quad(quad);
            }
            let mut y = bounds.top() - scroll_y;
            for (_, line) in &prepaint.lines {
                let height = line.size(line_height).height;
                if y + height >= bounds.top() && y <= bounds.bottom() {
                    let _ = line.paint(
                        point(bounds.left(), y),
                        line_height,
                        TextAlign::Left,
                        Some(bounds),
                        window,
                        cx,
                    );
                }
                y += height;
            }
            if focus_handle.is_focused(window) {
                if let Some(cursor) = prepaint.cursor.take() {
                    window.paint_quad(cursor);
                }
            }
        });
    }
}

impl Render for TextArea {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus_handle(cx))
            .cursor(CursorStyle::IBeam)
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::up))
            .on_action(cx.listener(Self::down))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_up))
            .on_action(cx.listener(Self::select_down))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::end))
            .on_action(cx.listener(Self::newline))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::copy))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_scroll_wheel(cx.listener(Self::on_scroll))
            .child(AreaElement { input: cx.entity() })
    }
}

impl Focusable for TextArea {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}
