//! El estante: la tarjeta que aparece abajo a la derecha tras una captura,
//! como en Atic (`ShelfSurface.svelte`, `capture_shelf.rs`).
//!
//! La captura vuela desde donde se tomó hasta la tarjeta. Encima de la tarjeta
//! aparecen las opciones: descartar, carpeta, Copiar, Dibujar y Texto. Clic en
//! la miniatura la abre; arrastrarla la suelta en otra app. Se va sola a los
//! 20 s, y la cuenta se pausa con el cursor encima.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use atic_capture::Frame;
use gpui::{
    div, img, prelude::*, px, rgb, svg, AnyElement, ClickEvent, Context, Hsla, Image,
    ImageFormat, MouseButton, MouseDownEvent, SharedString,
};

use crate::hover::HoverExt;
use crate::anim::{ease_smooth_out, lerp, segment};
use crate::capture::Saved;
use crate::geometry::Rect;

const CARD_W: f32 = 208.0;
const CARD_H: f32 = 136.0;
const PAD: f32 = 8.0;
const MARGIN: f32 = 16.0;
const FLY_MS: f32 = 340.0;
const LIFETIME: Duration = Duration::from_secs(20);
/// Mover esto con el botón apretado sobre la miniatura es arrastrarla.
const DRAG_START: f32 = 6.0;

pub struct Shelf {
    image: Arc<Image>,
    path: PathBuf,
    frame: Frame,
    /// Dónde se tomó, en la ventana: de ahí sale volando.
    from: Rect,
    born: Instant,
    /// Tiempo de vida que queda; solo corre sin el cursor encima.
    left: Duration,
    last_tick: Instant,
    hovered: bool,
    press: Option<(f32, f32)>,
    note: Option<(SharedString, Instant)>,
    busy: bool,
}

impl crate::Pill {
    pub(crate) fn show_shelf(&mut self, saved: Saved, from: Rect) {
        let now = Instant::now();
        crate::debug(|| {
            format!(
                "estante: {}×{} desde {from:?} a {:?} (trabajo {:?})",
                saved.width,
                saved.height,
                self.card_rect(),
                self.work
            )
        });
        self.shelf = Some(Shelf {
            image: Arc::new(Image::from_bytes(ImageFormat::Png, saved.png)),
            path: saved.path,
            frame: saved.frame,
            from,
            born: now,
            left: LIFETIME,
            last_tick: now,
            hovered: false,
            press: None,
            note: None,
            busy: false,
        });
    }

    /// `PILL_OPEN=shelf`: el estante con la última captura guardada, para
    /// revisarlo sin capturar.
    pub(crate) fn demo_shelf(&mut self, cx: &mut Context<Self>) {
        let Some(dir) = std::env::var_os("LOCALAPPDATA")
            .map(|base| PathBuf::from(base).join("atic-gpui").join("captures"))
        else {
            return;
        };
        let newest = std::fs::read_dir(&dir).ok().and_then(|entries| {
            entries
                .flatten()
                .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "png"))
                .max_by_key(|entry| entry.metadata().and_then(|m| m.modified()).ok())
                .map(|entry| entry.path())
        });
        let Some(path) = newest else {
            eprintln!("estante: no hay capturas en {}", dir.display());
            return;
        };
        let png = match std::fs::read(&path) {
            Ok(png) => png,
            Err(error) => return eprintln!("estante: {}: {error}", path.display()),
        };
        let frame = match Frame::from_png(0, 0, &png) {
            Ok(frame) => frame,
            Err(error) => return eprintln!("estante: PNG ilegible: {error}"),
        };
        let (width, height) = (frame.width(), frame.height());
        let scale = self.scale_factor;
        let from = Rect::new(
            self.work.x + self.work.w / 2.0 - width as f32 / scale / 4.0,
            self.work.y + self.work.h / 2.0 - height as f32 / scale / 4.0,
            width as f32 / scale / 2.0,
            height as f32 / scale / 2.0,
        );
        self.show_shelf(
            Saved {
                path,
                png,
                width,
                height,
                frame,
            },
            from,
        );
        cx.notify();
    }

    fn card_rect(&self) -> Rect {
        Rect::new(
            self.work.right() - CARD_W - MARGIN,
            self.work.bottom() - CARD_H - MARGIN,
            CARD_W,
            CARD_H,
        )
    }

    fn thumb_rect(&self) -> Rect {
        let card = self.card_rect();
        Rect::new(card.x + PAD, card.y + PAD, CARD_W - PAD * 2.0, CARD_H - PAD * 2.0)
    }

    /// Cada sondeo: cuenta regresiva, hover y arrastre. Devuelve si el cursor
    /// está sobre la tarjeta (la ventana tiene que recibir los clics).
    pub(crate) fn shelf_tick(&mut self, cursor: Option<(f32, f32)>, cx: &mut Context<Self>) -> bool {
        let card = self.card_rect();
        let Some(shelf) = self.shelf.as_mut() else {
            return false;
        };
        let now = Instant::now();
        let over = cursor.is_some_and(|c| card.contains(c, 0.0));
        let elapsed = now.duration_since(shelf.last_tick);
        shelf.last_tick = now;
        shelf.hovered = over;
        if !over && shelf.press.is_none() && !shelf.busy {
            shelf.left = shelf.left.saturating_sub(elapsed);
        }
        // Arrastrar la miniatura a otra app: el PNG como archivo.
        if let (Some(origin), Some(c)) = (shelf.press, cursor) {
            if (c.0 - origin.0).hypot(c.1 - origin.1) >= DRAG_START {
                shelf.press = None;
                let path = shelf.path.to_string_lossy().into_owned();
                cx.spawn(async move |this, cx| {
                    match crate::drag::drag_files(&[path]) {
                        Ok(outcome) if outcome.dropped => {
                            let _ = this.update(cx, |pill, cx| {
                                pill.shelf = None;
                                cx.notify();
                            });
                        }
                        Ok(_) => {}
                        Err(error) => eprintln!("estante: arrastre: {error}"),
                    }
                })
                .detach();
            }
        }
        if !crate::win::left_button_down() {
            shelf.press = None;
        }
        if shelf.left.is_zero() {
            self.shelf = None;
            cx.notify();
            return false;
        }
        over
    }

    pub(crate) fn shelf_animating(&self, now: Instant) -> bool {
        self.shelf
            .as_ref()
            .is_some_and(|shelf| now.duration_since(shelf.born).as_secs_f32() * 1000.0 < FLY_MS)
    }

    fn shelf_note(&mut self, text: &str, cx: &mut Context<Self>) {
        if let Some(shelf) = self.shelf.as_mut() {
            shelf.note = Some((text.to_string().into(), Instant::now()));
        }
        cx.notify();
    }

    fn shelf_copy(&mut self, cx: &mut Context<Self>) {
        let Some(shelf) = self.shelf.as_ref() else {
            return;
        };
        let frame = shelf.frame.clone();
        let png = shelf.image.bytes().to_vec();
        match crate::clip_image::write(frame.width(), frame.height(), &frame.bgra, &png) {
            Ok(()) => {
                // Copiar es lo que más se hace: copia y se va.
                self.shelf = None;
                println!("estante: copiada");
            }
            Err(error) => self.shelf_note(&format!("No se pudo copiar: {error}"), cx),
        }
        cx.notify();
    }

    fn shelf_draw(&mut self, window: &mut gpui::Window, cx: &mut Context<Self>) {
        let Some(shelf) = self.shelf.take() else {
            return;
        };
        let offset = (self.monitor.x, self.monitor.y);
        let origin = (shelf.from.x - offset.0, shelf.from.y - offset.1);
        let Some(frozen) = crate::board::frozen_for(shelf.frame, self.scale_factor, offset) else {
            return;
        };
        let previous = crate::paste::foreground_target();
        self.open_board(frozen, origin, previous, window, cx);
    }

    fn shelf_text(&mut self, cx: &mut Context<Self>) {
        let Some(shelf) = self.shelf.as_mut() else {
            return;
        };
        if shelf.busy {
            return;
        }
        shelf.busy = true;
        shelf.note = Some(("Leyendo…".into(), Instant::now()));
        let frame = shelf.frame.clone();
        cx.spawn(async move |this, cx| {
            let text = cx
                .background_spawn(async move {
                    crate::ocr::recognize(frame.width(), frame.height(), &frame.bgra)
                })
                .await;
            let _ = this.update(cx, |pill, cx| {
                if let Some(shelf) = pill.shelf.as_mut() {
                    shelf.busy = false;
                }
                match text {
                    Ok(text) if !text.trim().is_empty() => {
                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
                        pill.shelf_note("Texto copiado", cx);
                    }
                    Ok(_) => pill.shelf_note("No encontré texto", cx),
                    Err(error) => {
                        eprintln!("estante: OCR: {error}");
                        pill.shelf_note("No se pudo leer el texto", cx);
                    }
                }
            });
        })
        .detach();
        cx.notify();
    }

    fn shelf_open(&mut self) {
        if let Some(shelf) = self.shelf.as_ref() {
            let _ = std::process::Command::new("explorer").arg(&shelf.path).spawn();
        }
    }

    fn shelf_folder(&mut self) {
        if let Some(shelf) = self.shelf.as_ref() {
            let _ = std::process::Command::new("explorer")
                .arg(format!("/select,{}", shelf.path.display()))
                .spawn();
        }
    }

    pub(crate) fn render_shelf(&self, now: Instant, cx: &mut Context<Self>) -> Option<AnyElement> {
        let shelf = self.shelf.as_ref()?;
        let card = self.card_rect();
        let thumb = self.thumb_rect();
        let t = ease_smooth_out(segment(
            now.duration_since(shelf.born).as_secs_f32() * 1000.0,
            0.0,
            FLY_MS,
        ));
        // La foto viaja de la selección a la miniatura; la tarjeta aparece al
        // final, cuando la foto ya llegó.
        let fly = Rect::new(
            lerp(shelf.from.x, thumb.x, t),
            lerp(shelf.from.y, thumb.y, t),
            lerp(shelf.from.w, thumb.w, t),
            lerp(shelf.from.h, thumb.h, t),
        );
        let card_alpha = segment(t, 0.7, 0.3);
        let text: Hsla = rgb(0xf0f0ea).into();
        let chip_bg = gpui::black().opacity(0.55);
        let veil = shelf.hovered && shelf.note.is_none() && t >= 1.0;

        let dot = |id: &'static str, icon: &'static str| {
            div()
                .id(id)
                .absolute()
                .size(px(28.))
                .rounded(px(14.))
                .flex()
                .items_center()
                .justify_center()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(svg().path(icon).size(px(14.)).text_color(text))
        };
        let action = |id: &'static str, icon: &'static str, label: &'static str| {
            // `.shelf-sub` de Atic: botón de 26 px con un filo claro.
            div()
                .id(id)
                .h(px(26.))
                .px(px(12.))
                .flex()
                .items_center()
                .justify_center()
                .gap(px(6.))
                .rounded(px(8.))
                .border_1()
                .border_color(gpui::white().opacity(0.16))
                .text_size(px(12.))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(text)
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(svg().path(icon).size(px(13.)).text_color(text))
                .child(label)
        };

        let frame = div()
            .absolute()
            .left(px(card.x))
            .top(px(card.y))
            .w(px(CARD_W))
            .h(px(CARD_H))
            .rounded(px(14.))
            .bg(rgb(0x1a1a18))
            .shadow_lg()
            .opacity(card_alpha);
        let photo = div()
            .id("shelf-thumb")
            .absolute()
            .left(px(fly.x))
            .top(px(fly.y))
            .w(px(fly.w))
            .h(px(fly.h))
            .rounded(px(lerp(2.0, 8.0, t)))
            .overflow_hidden()
            .cursor_pointer()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|pill, event: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    if let Some(shelf) = pill.shelf.as_mut() {
                        shelf.press = Some((f32::from(event.position.x), f32::from(event.position.y)));
                    }
                }),
            )
            .on_click(cx.listener(|pill, _: &ClickEvent, _, _| pill.shelf_open()))
            .child(img(shelf.image.clone()).size_full().object_fit(gpui::ObjectFit::Cover));

        let progress = shelf.left.as_secs_f32() / LIFETIME.as_secs_f32();
        let overlay = div()
            .absolute()
            .left(px(thumb.x))
            .top(px(thumb.y))
            .w(px(thumb.w))
            .h(px(thumb.h))
            .rounded(px(8.))
            .when(veil, |el| {
                el.bg(gpui::black().opacity(0.4))
                    .child(
                        dot("shelf-dismiss", "icons/x.svg")
                            .tooltip(crate::hover::tip("Descartar"))
                            .left(px(6.))
                            .top(px(6.))
                            .on_click(cx.listener(|pill, _: &ClickEvent, _, cx| {
                                pill.shelf = None;
                                cx.notify();
                            }))
                            .hover_bg("shelf-dismiss-fx", chip_bg, gpui::black().opacity(0.8)),
                    )
                    .child(
                        dot("shelf-folder", "icons/folder.svg")
                            .tooltip(crate::hover::tip("Abrir la carpeta"))
                            .right(px(6.))
                            .top(px(6.))
                            .on_click(cx.listener(|pill, _: &ClickEvent, _, _| pill.shelf_folder()))
                            .hover_bg("shelf-folder-fx", chip_bg, gpui::black().opacity(0.8)),
                    )
                    .child(
                        // `.shelf-center`: una columna al centro de la foto.
                        div()
                            .absolute()
                            .top(px((thumb.h - (26.0 * 3.0 + 5.0 * 2.0)) / 2.0))
                            .left(px((thumb.w - 100.0) / 2.0))
                            .w(px(100.))
                            .flex()
                            .flex_col()
                            .gap(px(5.))
                            .child(
                                action("shelf-copy", "icons/copy.svg", "Copiar")
                                    .on_click(cx.listener(|pill, _: &ClickEvent, _, cx| pill.shelf_copy(cx)))
                                    .hover_bg("shelf-copy-fx", chip_bg, gpui::black().opacity(0.8)),
                            )
                            .child(
                                action("shelf-draw", "icons/pencil.svg", "Dibujar")
                                    .on_click(cx.listener(|pill, _: &ClickEvent, window, cx| {
                                        pill.shelf_draw(window, cx)
                                    }))
                                    .hover_bg("shelf-draw-fx", chip_bg, gpui::black().opacity(0.8)),
                            )
                            .child(
                                action("shelf-text", "icons/scan-text.svg", "Texto")
                                    .on_click(cx.listener(|pill, _: &ClickEvent, _, cx| pill.shelf_text(cx)))
                                    .hover_bg("shelf-text-fx", chip_bg, gpui::black().opacity(0.8)),
                            ),
                    )
            })
            .when_some(shelf.note.as_ref(), |el, (note, _)| {
                el.flex().items_end().justify_center().pb(px(10.)).child(
                    div()
                        .px(px(10.))
                        .py(px(4.))
                        .rounded(px(10.))
                        .bg(gpui::black().opacity(0.75))
                        .text_size(px(11.))
                        .text_color(text)
                        .child(note.clone()),
                )
            });
        let countdown = div()
            .absolute()
            .left(px(card.x + 14.0))
            .top(px(card.bottom() - 4.0))
            .w(px((CARD_W - 28.0) * progress))
            .h(px(2.))
            .rounded(px(1.))
            .bg(text.opacity(0.35))
            .opacity(card_alpha);
        let tip = (veil).then(|| {
            div()
                .absolute()
                .left(px(card.x))
                .top(px(card.y - 26.0))
                .w(px(CARD_W))
                .flex()
                .justify_center()
                .child(
                    div()
                        .px(px(8.))
                        .py(px(3.))
                        .rounded(px(8.))
                        .bg(gpui::black().opacity(0.78))
                        .text_size(px(10.))
                        .text_color(text)
                        .child("Clic: abrir · Arrastra para soltar"),
                )
        });
        Some(
            // Sin `top`/`left`, un absoluto queda donde iría en el flujo: bajo el
            // canvas de pantalla completa, o sea fuera de la vista.
            div()
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .font_family("Segoe UI")
                .child(frame)
                .child(photo)
                .when(t >= 1.0, |el| el.child(overlay).child(countdown))
                .children(tip)
                .into_any_element(),
        )
    }
}
