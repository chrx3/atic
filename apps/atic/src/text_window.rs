//! «Texto»: el texto de una captura en una ventana chica, como «Extract Text»
//! de CleanShot. Lo abre el botón «Texto» del estante (`shelf.rs`).
//!
//! El contenido es el mismo de la pestaña Texto del editor de capturas
//! (`extract.rs`): el texto leído con el OCR del sistema y su traducción al
//! lado con «Traducir» (con IA o la rápida del Mac, `translate.rs`). Arriba,
//! «Fijar» deja la ventana encima de las demás.

mod extract;
mod translate;

pub use extract::{translates, TextExtract, TranslationToggled};

use std::sync::Arc;

use atic_capture::Frame;
use gpui::{
    div, prelude::*, px, size, App, Bounds, ClickEvent, Context, Entity, FontWeight, Window, WindowBounds, WindowKind,
    WindowOptions,
};
use gpui_m3::theme::type_scale::{self, TypeScaleExt};

use crate::i18n::t;
use crate::kit::chrome;

const TOP_H: f32 = 40.0;
const GUTTER: f32 = 16.0;

/// El tamaño de la ventana: con traducción, ancha para los dos lados.
fn window_size() -> (f32, f32) {
    if translates() { (760.0, 480.0) } else { (440.0, 360.0) }
}

/// Abre la ventana con el texto de `png` (o trae al frente la que ya lo muestra).
pub fn open(png: Arc<Vec<u8>>, cx: &mut App) {
    let existing = cx
        .windows()
        .into_iter()
        .filter_map(|w| w.downcast::<TextWindow>())
        .find(|handle| handle.read(cx).is_ok_and(|view| Arc::ptr_eq(&view.png, &png)));
    if let Some(handle) = existing {
        if handle.update(cx, |_, window, _| crate::win::bring_to_front(window)).is_ok() {
            return;
        }
    }
    let (w, h) = window_size();
    let min = if translates() { size(px(520.), px(300.)) } else { size(px(320.), px(220.)) };
    let options = WindowOptions {
        titlebar: Some(chrome::titlebar(t("pill.textWindow.title"), TOP_H)),
        window_min_size: Some(min),
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(None, size(px(w), px(h)), cx))),
        focus: crate::win::FOCUS_ON_OPEN,
        show: true,
        kind: WindowKind::Normal,
        ..Default::default()
    };
    let opened = cx.open_window(options, |window, cx| {
        chrome::setup(window);
        cx.new(|cx| TextWindow::new(png, cx))
    });
    match opened {
        Ok(handle) => {
            let _ = handle.update(cx, |_, window, _| crate::win::bring_to_front(window));
        }
        Err(error) => tracing::warn!(%error, "texto: no se pudo abrir la ventana"),
    }
}

pub struct TextWindow {
    png: Arc<Vec<u8>>,
    extract: Entity<TextExtract>,
    pinned: bool,
    backdrop: Option<bool>,
}

impl TextWindow {
    fn new(png: Arc<Vec<u8>>, cx: &mut Context<Self>) -> Self {
        let image = png.clone();
        let extract = cx.new(|cx| TextExtract::new(move || Frame::from_png(0, 0, &image).map_err(|error| error.to_string()), cx));
        Self { png, extract, pinned: false, backdrop: None }
    }

    fn toggle_pin(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.pinned = !self.pinned;
        crate::win::set_always_on_top(window, self.pinned);
        cx.notify();
    }
}

impl Render for TextWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::theme::sync_backdrop(window, &mut self.backdrop);
        let m3_scope = crate::theme::scope_m3(cx);
        let text = crate::theme::expressive().map_or_else(|| crate::theme::hsla(crate::theme::TEXT), |t| t.text);

        let pin = gpui_m3::IconButton::new("text-window-pin", "icons/pin.svg")
            .size(px(32.))
            .selected(self.pinned)
            .tooltip(if self.pinned { t("pill.textWindow.unpin") } else { t("pill.textWindow.pin") })
            .on_click(cx.listener(|view, _: &ClickEvent, window, cx| view.toggle_pin(window, cx)));
        let top = div()
            .h(px(TOP_H))
            .flex_none()
            .pl(px(GUTTER - 8.0))
            .flex()
            .items_center()
            .gap(px(8.))
            .child(chrome::lights())
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .type_style(type_scale::TITLE_SMALL)
                    .child(t("pill.textWindow.title")),
            )
            .child(chrome::drag(TOP_H))
            .child(div().flex_none().pr(px(if cfg!(target_os = "macos") { 8. } else { 0. })).child(pin))
            .child(chrome::controls(window.is_maximized(), TOP_H));

        let pieces = crate::glass_pieces::active_windows();
        div()
            .size_full()
            .flex()
            .flex_col()
            .map(|el| crate::theme::window_frame(el, window))
            .when(!pieces, |el| el.bg(crate::theme::window_bg()))
            .when(pieces, |el| {
                el.child(crate::glass_pieces::slot(crate::glass_pieces::Piece::Fill { radius: 0., color: crate::theme::window_bg() }))
            })
            .font_family(crate::theme::font())
            .text_color(text)
            .child(top)
            .child(div().flex_1().min_h_0().flex().flex_col().px(px(GUTTER - 4.0)).pb(px(GUTTER - 4.0)).child(self.extract.clone()))
            .child(crate::glass_pieces::commit(crate::theme::is_light()))
            .children(m3_scope)
    }
}
