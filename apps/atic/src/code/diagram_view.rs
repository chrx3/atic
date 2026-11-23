//! El visor de un diagrama Mermaid de una respuesta o de una imagen del panel: a pantalla completa sobre Atic Code,
//! con zoom (rueda o botones; clic en el porcentaje vuelve a ajustarlo a la ventana),
//! arrastre para moverlo, copiar la imagen y guardarla como PNG. Esc o la x lo cierran.

use std::path::{Path, PathBuf};

use gpui::prelude::FluentBuilder;
use gpui::{
    div, hsla, img, px, ClickEvent, Context, ImageSource, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, MouseMoveEvent,
    ParentElement, Pixels, Point, ScrollWheelEvent, SharedString, StatefulInteractiveElement, Styled, Window,
};

use super::view::icon_button;
use super::CodeView;
use crate::i18n::{t, tf};

/// Lo más chico y lo más grande que se puede ver (1 = su tamaño natural).
const MIN_ZOOM: f32 = 0.1;
const MAX_ZOOM: f32 = 8.;
/// Cuánto cambia el zoom por paso (botón o clic de rueda).
const STEP: f32 = 1.25;
/// El margen alrededor del diagrama ajustado a la ventana (arriba va la barra).
const MARGIN: f32 = 48.;
const BAR: f32 = 64.;

/// Lo que muestra el visor.
pub(super) enum Shown {
    Diagram(gpui_m3::MermaidDiagram),
    /// Una imagen del disco (la del panel derecho), a su tamaño original.
    File(PathBuf),
}

pub(super) struct DiagramView {
    pub shown: Shown,
    /// Su tamaño natural en píxeles lógicos.
    pub size: (f32, f32),
    /// `None`: ajustado a la ventana.
    pub zoom: Option<f32>,
    /// Cuánto se movió el centro del diagrama desde el centro de la ventana.
    pub offset: Point<Pixels>,
    /// Dónde iba el puntero en el último cuadro de un arrastre.
    pub drag: Option<Point<Pixels>>,
}

impl DiagramView {
    pub fn new(diagram: gpui_m3::MermaidDiagram) -> Self {
        Self { size: diagram.size, shown: Shown::Diagram(diagram), zoom: None, offset: Point::default(), drag: None }
    }

    fn file(path: PathBuf, size: (f32, f32)) -> Self {
        Self { shown: Shown::File(path), size, zoom: None, offset: Point::default(), drag: None }
    }
}

/// El ancho y el alto de una imagen del disco, mirando solo su cabecera.
fn image_dimensions_of(path: &Path) -> Option<(u32, u32)> {
    image::image_dimensions(path).ok()
}

/// Pone la imagen del archivo en el portapapeles; `false` si no se pudo leer.
fn copy_image_file(path: &Path, cx: &mut gpui::App) -> bool {
    let format = match super::image_type(path) {
        Some("image/png") => gpui::ImageFormat::Png,
        Some("image/jpeg") => gpui::ImageFormat::Jpeg,
        Some("image/gif") => gpui::ImageFormat::Gif,
        Some("image/webp") => gpui::ImageFormat::Webp,
        _ => return false,
    };
    let Ok(bytes) = std::fs::read(path) else {
        return false;
    };
    cx.write_to_clipboard(gpui::ClipboardItem::new_image(&gpui::Image::from_bytes(format, bytes)));
    true
}

/// El zoom que hace caber `size` en `room` (sin agrandar más del doble un diagrama chico).
pub(super) fn fit_zoom(size: (f32, f32), room: (f32, f32)) -> f32 {
    let (w, h) = (size.0.max(1.), size.1.max(1.));
    let (rw, rh) = ((room.0 - 2. * MARGIN).max(80.), (room.1 - 2. * MARGIN - BAR).max(80.));
    (rw / w).min(rh / h).clamp(MIN_ZOOM, 2.)
}

/// Un paso de zoom desde `zoom`, hacia adentro (`steps > 0`) o hacia afuera.
pub(super) fn step_zoom(zoom: f32, steps: f32) -> f32 {
    (zoom * STEP.powf(steps)).clamp(MIN_ZOOM, MAX_ZOOM)
}

impl CodeView {
    /// Abre el visor con un diagrama (lo llama el `Markdown` de gpui-m3 al ampliarlo).
    pub(super) fn open_diagram(&mut self, diagram: &gpui_m3::MermaidDiagram, cx: &mut Context<Self>) {
        self.diagram = Some(DiagramView::new(diagram.clone()));
        cx.notify();
    }

    /// Abre en el visor una imagen del disco, con zoom y arrastre. Una que no se puede
    /// medir (formato desconocido) se queda como está en el panel.
    pub(super) fn open_image_viewer(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let Some((width, height)) = image_dimensions_of(&path) else {
            return;
        };
        self.diagram = Some(DiagramView::file(path, (width as f32, height as f32)));
        cx.notify();
    }

    pub(super) fn close_diagram(&mut self, cx: &mut Context<Self>) {
        if self.diagram.take().is_some() {
            cx.notify();
        }
    }

    fn diagram_zoom(&self, window: &Window) -> f32 {
        let Some(view) = &self.diagram else {
            return 1.;
        };
        view.zoom.unwrap_or_else(|| {
            let room = window.viewport_size();
            fit_zoom(view.size, (f32::from(room.width), f32::from(room.height)))
        })
    }

    /// Cambia el zoom dejando quieto el punto del diagrama bajo `at` (el puntero), o el centro.
    fn zoom_diagram(&mut self, steps: f32, at: Option<Point<Pixels>>, window: &Window, cx: &mut Context<Self>) {
        let old = self.diagram_zoom(window);
        let new = step_zoom(old, steps);
        let room = window.viewport_size();
        let center = Point::new(room.width / 2., room.height / 2.);
        let Some(view) = self.diagram.as_mut() else {
            return;
        };
        if let Some(at) = at {
            let k = new / old;
            let rel = at - center - view.offset;
            view.offset = at - center - Point::new(rel.x * k, rel.y * k);
        } else {
            view.offset = Point::new(view.offset.x * (new / old), view.offset.y * (new / old));
        }
        view.zoom = Some(new);
        cx.notify();
    }

    fn fit_diagram(&mut self, cx: &mut Context<Self>) {
        if let Some(view) = self.diagram.as_mut() {
            view.zoom = None;
            view.offset = Point::default();
            cx.notify();
        }
    }

    fn save_diagram(&mut self, cx: &mut Context<Self>) {
        let Some(DiagramView { shown: Shown::Diagram(diagram), .. }) = &self.diagram else {
            return;
        };
        let png = diagram.png.clone();
        let dir = self.active_chat().map(|c| c.cwd.clone()).unwrap_or_else(std::env::temp_dir);
        let picked = cx.prompt_for_new_path(&dir, Some("diagrama.png"));
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(path))) = picked.await else {
                return;
            };
            let result = std::fs::write(&path, png.as_slice());
            let _ = this.update(cx, |view, cx| match result {
                Ok(()) => view.show_toast(t("pill.code.diagram.saved"), cx),
                Err(error) => view.show_toast(tf("pill.code.diagram.saveFailed", &[("error", &error)]), cx),
            });
        })
        .detach();
    }

    /// La capa del visor, si hay un diagrama abierto.
    pub(super) fn diagram_layer(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let view = self.diagram.as_ref()?;
        let scheme = *gpui_m3::Theme::of(cx);
        let zoom = self.diagram_zoom(window);
        let (w, h) = view.size;
        let room = window.viewport_size();
        let left = room.width / 2. + view.offset.x - px(w * zoom / 2.);
        let top = room.height / 2. + view.offset.y - px(h * zoom / 2.);
        let pct = (zoom * 100.).round() as u32;
        let dragging = view.drag.is_some();

        let label: SharedString = format!("{pct} %").into();
        let bar = div()
            .absolute()
            .top(px(16.))
            .left_0()
            .right_0()
            .flex()
            .justify_center()
            .child(
                div()
                    .id("diagram-bar")
                    .occlude()
                    .flex()
                    .items_center()
                    .gap(px(2.))
                    .p(px(4.))
                    .rounded(px(16.))
                    .bg(scheme.surface_container_high)
                    .text_color(scheme.on_surface)
                    .child(icon_button("diagram-out", "icons/minus.svg", t("pill.code.diagram.zoomOut")).on_click(cx.listener(|view, _: &ClickEvent, window, cx| view.zoom_diagram(-1., None, window, cx))))
                    .child(
                        div()
                            .id("diagram-fit")
                            .min_w(px(56.))
                            .h(px(28.))
                            .px(px(6.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(12.))
                            .text_size(px(12.5))
                            .cursor_pointer()
                            .hover(|el| el.bg(scheme.surface_container_highest))
                            .tooltip(crate::hover::tip(t("pill.code.diagram.fit")))
                            .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.fit_diagram(cx)))
                            .child(label),
                    )
                    .child(icon_button("diagram-in", "icons/plus.svg", t("pill.code.diagram.zoomIn")).on_click(cx.listener(|view, _: &ClickEvent, window, cx| view.zoom_diagram(1., None, window, cx))))
                    .child(div().w(px(1.)).h(px(18.)).mx(px(4.)).bg(scheme.outline_variant))
                    .child(icon_button("diagram-copy", "icons/copy.svg", t("pill.code.diagram.copy")).on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                        let copied = match view.diagram.as_ref().map(|open| &open.shown) {
                            Some(Shown::Diagram(diagram)) => {
                                diagram.copy(cx);
                                true
                            }
                            Some(Shown::File(path)) => copy_image_file(path, cx),
                            None => false,
                        };
                        if copied {
                            view.show_toast(t("pill.code.diagram.copied"), cx);
                        }
                    })))
                    .when(matches!(view.shown, Shown::Diagram(_)), |bar| {
                        bar.child(icon_button("diagram-save", "icons/download.svg", t("pill.code.diagram.save")).on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.save_diagram(cx))))
                    })
                    .child(icon_button("diagram-close", "icons/x.svg", t("pill.code.diagram.close")).on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.close_diagram(cx)))),
            );

        Some(
            div()
                .id("diagram-viewer")
                .absolute()
                .inset_0()
                .occlude()
                .overflow_hidden()
                .bg(hsla(0., 0., 0., 0.8))
                .when(dragging, |el| el.cursor_grabbing())
                .when(!dragging, |el| el.cursor_grab())
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|view, event: &MouseDownEvent, _, cx| {
                        if let Some(open) = view.diagram.as_mut() {
                            open.drag = Some(event.position);
                            cx.notify();
                        }
                    }),
                )
                .on_mouse_move(cx.listener(|view, event: &MouseMoveEvent, _, cx| {
                    let Some(open) = view.diagram.as_mut() else {
                        return;
                    };
                    let Some(last) = open.drag else {
                        return;
                    };
                    if event.pressed_button != Some(MouseButton::Left) {
                        open.drag = None;
                        cx.notify();
                        return;
                    }
                    open.offset = open.offset + (event.position - last);
                    open.drag = Some(event.position);
                    cx.notify();
                }))
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|view, _, _, cx| {
                        if let Some(open) = view.diagram.as_mut() {
                            open.drag = None;
                            cx.notify();
                        }
                    }),
                )
                .on_scroll_wheel(cx.listener(|view, event: &ScrollWheelEvent, window, cx| {
                    let dy = f32::from(event.delta.pixel_delta(px(40.)).y);
                    if dy.abs() < 0.5 {
                        return;
                    }
                    // Un clic de rueda (~40 px) es un paso; el trackpad va más fino.
                    view.zoom_diagram(dy / 40., Some(event.position), window, cx);
                }))
                .child(
                    div()
                        .absolute()
                        .left(left)
                        .top(top)
                        .w(px(w * zoom))
                        .h(px(h * zoom))
                        .rounded(px(12.))
                        .overflow_hidden()
                        .child(match &view.shown {
                            Shown::Diagram(diagram) => img(ImageSource::Render(diagram.image.clone())).size_full(),
                            Shown::File(path) => img(path.clone()).size_full(),
                        }),
                )
                .child(bar)
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_diagrama_cabe_en_la_ventana() {
        // Ancho: manda el ancho.
        let z = fit_zoom((2000., 500.), (1000., 800.));
        assert!((2000. * z) <= 1000. - 2. * MARGIN + 0.5);
        // Chico: se agranda, pero no más del doble.
        assert_eq!(fit_zoom((100., 50.), (1000., 800.)), 2.);
    }

    #[test]
    fn el_zoom_tiene_topes() {
        assert_eq!(step_zoom(1., 1.), STEP);
        assert_eq!(step_zoom(MAX_ZOOM, 3.), MAX_ZOOM);
        assert_eq!(step_zoom(MIN_ZOOM, -3.), MIN_ZOOM);
    }
}
