//! El estante: las capturas recientes, flotando abajo a la derecha, como en
//! Atic (`ShelfSurface.svelte`, `capture_shelf.rs`).
//!
//! La captura vuela desde donde se tomó hasta su lugar. Las nuevas entran
//! abajo y empujan a las anteriores hacia arriba. Con el cursor encima de una
//! aparecen sus opciones: descartar, carpeta, Copiar, Dibujar y Texto. Clic en
//! la foto la abre; arrastrarla la suelta en otra app. Cada una se va
//! achicando durante 20 s y al vencer hace «pop»; con el cursor encima vuelve
//! a su tamaño y la cuenta se pausa.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use atic_capture::Frame;
use gpui::{
    div, img, point, prelude::*, px, rgb, svg, AnyElement, BoxShadow, ClickEvent, Context, Hsla,
    Image, ImageFormat, MouseButton, MouseDownEvent, SharedString,
};

use crate::anim::{ease_island, ease_smooth_out, lerp, segment, Tween};
use crate::capture::Saved;
use crate::geometry::Rect;
use crate::hover::HoverExt;

const THUMB_W: f32 = 192.0;
const THUMB_H: f32 = 120.0;
/// Entre una foto y la de arriba.
const GAP: f32 = 10.0;
const MARGIN: f32 = 16.0;
const FLY_MS: f32 = 340.0;
/// Lo que tarda una foto en subir cuando entra otra o se va una.
const SLIDE: Duration = Duration::from_millis(320);
const LIFETIME: Duration = Duration::from_secs(20);
/// Hasta dónde se achica al acercarse el vencimiento.
const END_SCALE: f32 = 0.6;
/// El «pop» del final: crece un poco y se desvanece.
const POP: Duration = Duration::from_millis(200);
const POP_GROW: f32 = 0.18;
/// Mover esto con el botón apretado sobre la miniatura es arrastrarla.
const DRAG_START: f32 = 6.0;

pub struct Shelf {
    /// Identifica la foto aunque cambie de lugar en la pila.
    id: u64,
    image: Arc<Image>,
    path: PathBuf,
    frame: Frame,
    /// Dónde se tomó, en la ventana: de ahí sale volando.
    from: Rect,
    born: Instant,
    /// Su lugar en la pila (0 abajo), animado al subir.
    slot: Tween,
    /// Con el cursor encima vuelve a su tamaño (0 achicada, 1 entera).
    grow: Tween,
    /// Venció: desde cuándo hace «pop».
    popped: Option<Instant>,
    /// Tiempo de vida que queda; solo corre sin el cursor encima.
    left: Duration,
    last_tick: Instant,
    hovered: bool,
    press: Option<(f32, f32)>,
    /// El botón izquierdo en el sondeo anterior: para ver cuándo se aprieta.
    was_down: bool,
    note: Option<(SharedString, Instant)>,
    busy: bool,
}

impl crate::Pill {
    pub(crate) fn show_shelf(&mut self, saved: Saved, from: Rect) {
        let now = Instant::now();
        crate::debug(|| format!("estante: {}×{} desde {from:?}", saved.width, saved.height));
        self.shelf_seq += 1;
        self.shelves.insert(
            0,
            Shelf {
                id: self.shelf_seq,
                image: Arc::new(Image::from_bytes(ImageFormat::Png, saved.png)),
                path: saved.path,
                frame: saved.frame,
                from,
                born: now,
                slot: Tween::new(0.0, SLIDE, ease_island),
                grow: Tween::new(0.0, Duration::from_millis(200), ease_smooth_out),
                popped: None,
                left: LIFETIME,
                last_tick: now,
                hovered: false,
                press: None,
                was_down: false,
                note: None,
                busy: false,
            },
        );
        // Las que no caben en la altura de la pantalla se van.
        let fits = ((self.work.h - MARGIN) / (THUMB_H + GAP)).floor().max(1.0) as usize;
        self.shelves.truncate(fits);
        self.restack(now);
    }

    /// Cada foto, a su lugar en la pila.
    fn restack(&mut self, now: Instant) {
        for (index, shelf) in self.shelves.iter_mut().enumerate() {
            shelf.slot.set(index as f32, now);
        }
    }

    fn remove_shelf(&mut self, id: u64, cx: &mut Context<Self>) {
        self.shelves.retain(|shelf| shelf.id != id);
        self.restack(Instant::now());
        cx.notify();
    }

    fn shelf_mut(&mut self, id: u64) -> Option<&mut Shelf> {
        self.shelves.iter_mut().find(|shelf| shelf.id == id)
    }

    /// `PILL_OPEN=shelf`: el estante con la última captura guardada, para
    /// revisarlo sin capturar.
    pub(crate) fn demo_shelf(&mut self, cx: &mut Context<Self>) {
        let Some(dir) = crate::paths::captures_dir() else {
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

    /// La foto en el lugar `slot` de la pila (0 abajo; fraccionario al subir).
    fn thumb_rect(&self, slot: f32) -> Rect {
        Rect::new(
            self.work.right() - THUMB_W - MARGIN,
            self.work.bottom() - THUMB_H - MARGIN - slot * (THUMB_H + GAP),
            THUMB_W,
            THUMB_H,
        )
    }

    /// Cuánto mide ahora: se achica con el tiempo, vuelve con el cursor
    /// encima y al vencer crece un poco mientras se desvanece.
    fn photo_scale(shelf: &Shelf, now: Instant) -> f32 {
        let life = shelf.left.as_secs_f32() / LIFETIME.as_secs_f32();
        let aged = lerp(END_SCALE, 1.0, life);
        let scale = lerp(aged, 1.0, shelf.grow.value(now));
        match shelf.popped {
            Some(at) => scale * (1.0 + POP_GROW * Self::pop_progress(at, now)),
            None => scale,
        }
    }

    fn pop_progress(at: Instant, now: Instant) -> f32 {
        (now.duration_since(at).as_secs_f32() / POP.as_secs_f32()).clamp(0.0, 1.0)
    }

    /// La foto en su lugar, a su tamaño de ahora: pegada a la derecha y
    /// centrada en el alto de su lugar.
    fn photo_rect(&self, shelf: &Shelf, now: Instant) -> Rect {
        let slot = self.thumb_rect(shelf.slot.value(now));
        let scale = Self::photo_scale(shelf, now);
        let (w, h) = (slot.w * scale, slot.h * scale);
        Rect::new(slot.right() - w, slot.y + (slot.h - h) / 2.0, w, h)
    }

    /// Cada sondeo: cuenta regresiva, hover y arrastre. Devuelve si el cursor
    /// está sobre alguna foto (la ventana tiene que recibir los clics).
    pub(crate) fn shelf_tick(&mut self, cursor: Option<(f32, f32)>, cx: &mut Context<Self>) -> bool {
        if self.shelves.is_empty() {
            return false;
        }
        let now = Instant::now();
        let rects: Vec<Rect> = self.shelves.iter().map(|shelf| self.photo_rect(shelf, now)).collect();
        let button_down = crate::win::left_button_down();
        let mut over_any = false;
        let mut expired = Vec::new();
        for (shelf, rect) in self.shelves.iter_mut().zip(rects) {
            if let Some(at) = shelf.popped {
                if Self::pop_progress(at, now) >= 1.0 {
                    expired.push(shelf.id);
                }
                continue;
            }
            let over = cursor.is_some_and(|c| rect.contains(c, 0.0));
            over_any |= over;
            let elapsed = now.duration_since(shelf.last_tick);
            shelf.last_tick = now;
            shelf.hovered = over;
            shelf.grow.set(if over { 1.0 } else { 0.0 }, now);
            if !over && shelf.press.is_none() && !shelf.busy {
                shelf.left = shelf.left.saturating_sub(elapsed);
            }
            // Apretar en cualquier parte de la foto, también sobre Copiar,
            // Dibujar o Texto, que tapan casi todo el centro: esos botones
            // detienen el evento y la foto nunca se enteraba.
            if over && button_down && !shelf.was_down && shelf.press.is_none() {
                shelf.press = cursor;
            }
            shelf.was_down = button_down;
            // Arrastrar la miniatura a otra app: el PNG como archivo.
            if let (Some(origin), Some(c)) = (shelf.press, cursor) {
                if (c.0 - origin.0).hypot(c.1 - origin.1) >= DRAG_START {
                    shelf.press = None;
                    let (id, path) = (shelf.id, shelf.path.to_string_lossy().into_owned());
                    cx.spawn(async move |this, cx| match crate::drag::drag_files(&[path]) {
                        Ok(outcome) if outcome.dropped => {
                            let _ = this.update(cx, |pill, cx| pill.remove_shelf(id, cx));
                        }
                        Ok(_) => {}
                        Err(error) => tracing::warn!(%error, "estante: arrastre"),
                    })
                    .detach();
                }
            }
            if !button_down {
                shelf.press = None;
            }
            if shelf.left.is_zero() {
                shelf.popped = Some(now);
                shelf.hovered = false;
            }
        }
        if !expired.is_empty() {
            self.shelves.retain(|shelf| !expired.contains(&shelf.id));
            self.restack(now);
            cx.notify();
        }
        over_any
    }

    /// Solo lo rápido pide cuadros: el vuelo, la subida, el crecer con el
    /// cursor y el «pop». El achique es tan lento (unos 4 px/s) que le bastan
    /// los ~20 cuadros/s con que la pill ya respira.
    pub(crate) fn shelf_animating(&self, now: Instant) -> bool {
        self.shelves.iter().any(|shelf| {
            now.duration_since(shelf.born).as_secs_f32() * 1000.0 < FLY_MS
                || shelf.slot.is_running(now)
                || shelf.grow.is_running(now)
                || shelf.popped.is_some()
        })
    }

    fn shelf_note(&mut self, id: u64, text: &str, cx: &mut Context<Self>) {
        if let Some(shelf) = self.shelf_mut(id) {
            shelf.note = Some((text.to_string().into(), Instant::now()));
        }
        cx.notify();
    }

    fn shelf_copy(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(shelf) = self.shelf_mut(id) else {
            return;
        };
        let frame = shelf.frame.clone();
        let png = shelf.image.bytes().to_vec();
        match crate::clip_image::write(frame.width(), frame.height(), &frame.bgra, &png) {
            Ok(()) => {
                // Copiar es lo que más se hace: copia y se va.
                self.remove_shelf(id, cx);
                println!("estante: copiada");
            }
            Err(error) => self.shelf_note(id, &format!("No se pudo copiar: {error}"), cx),
        }
        cx.notify();
    }

    fn shelf_draw(&mut self, id: u64, window: &mut gpui::Window, cx: &mut Context<Self>) {
        let Some(index) = self.shelves.iter().position(|shelf| shelf.id == id) else {
            return;
        };
        let shelf = self.shelves.remove(index);
        self.restack(Instant::now());
        let previous = crate::paste::foreground_target();
        self.open_board_centered(shelf.frame, previous, window, cx);
    }

    fn shelf_text(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(shelf) = self.shelf_mut(id) else {
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
                if let Some(shelf) = pill.shelf_mut(id) {
                    shelf.busy = false;
                }
                match text {
                    Ok(text) if !text.trim().is_empty() => {
                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
                        pill.shelf_note(id, "Texto copiado", cx);
                    }
                    Ok(_) => pill.shelf_note(id, "No encontré texto", cx),
                    Err(error) => {
                        eprintln!("estante: OCR: {error}");
                        pill.shelf_note(id, "No se pudo leer el texto", cx);
                    }
                }
            });
        })
        .detach();
        cx.notify();
    }

    fn shelf_open(&mut self, id: u64) {
        if let Some(shelf) = self.shelf_mut(id) {
            let _ = std::process::Command::new("explorer").arg(&shelf.path).spawn();
        }
    }

    fn shelf_folder(&mut self, id: u64) {
        if let Some(shelf) = self.shelf_mut(id) {
            let _ = std::process::Command::new("explorer")
                .arg(format!("/select,{}", shelf.path.display()))
                .spawn();
        }
    }

    pub(crate) fn render_shelf(&self, now: Instant, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.shelves.is_empty() {
            return None;
        }
        let photos: Vec<AnyElement> = self.shelves.iter().map(|shelf| self.render_photo(shelf, now, cx)).collect();
        Some(
            // Sin `top`/`left`, un absoluto queda donde iría en el flujo: bajo el
            // canvas de pantalla completa, o sea fuera de la vista.
            div()
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .font_family("Segoe UI")
                .children(photos)
                .into_any_element(),
        )
    }

    fn render_photo(&self, shelf: &Shelf, now: Instant, cx: &mut Context<Self>) -> AnyElement {
        let id = shelf.id;
        let thumb = self.photo_rect(shelf, now);
        let fade = shelf.popped.map_or(1.0, |at| 1.0 - ease_smooth_out(Self::pop_progress(at, now)));
        let t = ease_smooth_out(segment(
            now.duration_since(shelf.born).as_secs_f32() * 1000.0,
            0.0,
            FLY_MS,
        ));
        // La foto viaja de la selección a su lugar en la pila.
        let fly = Rect::new(
            lerp(shelf.from.x, thumb.x, t),
            lerp(shelf.from.y, thumb.y, t),
            lerp(shelf.from.w, thumb.w, t),
            lerp(shelf.from.h, thumb.h, t),
        );
        let text: Hsla = rgb(0xf0f0ea).into();
        let chip_bg = gpui::black().opacity(0.55);
        let veil = shelf.hovered && shelf.note.is_none() && t >= 1.0;

        let dot = |name: &'static str, icon: &'static str| {
            div()
                .id((name, id as usize))
                .absolute()
                .size(px(28.))
                .rounded(px(14.))
                .flex()
                .items_center()
                .justify_center()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(svg().path(icon).size(px(14.)).text_color(text))
        };
        let action = |name: &'static str, icon: &'static str, label: &'static str| {
            // `.shelf-sub` de Atic: botón de 26 px con un filo claro.
            div()
                .id((name, id as usize))
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

        // Sin tarjeta: solo la foto, flotando. La sombra va en una capa propia:
        // la foto recorta lo que tiene dentro.
        let radius = px(lerp(2.0, 8.0, t));
        let shadow = div()
            .absolute()
            .left(px(fly.x))
            .top(px(fly.y))
            .w(px(fly.w))
            .h(px(fly.h))
            .rounded(radius)
            .opacity(fade)
            .shadow(vec![
                BoxShadow {
                    color: gpui::black().opacity(0.45 * t),
                    offset: point(px(0.), px(10.)),
                    blur_radius: px(28.),
                    spread_radius: px(0.),
                },
                BoxShadow {
                    color: gpui::black().opacity(0.35 * t),
                    offset: point(px(0.), px(2.)),
                    blur_radius: px(6.),
                    spread_radius: px(0.),
                },
            ]);
        let photo = div()
            .id(("shelf-thumb", id as usize))
            .absolute()
            .left(px(fly.x))
            .top(px(fly.y))
            .w(px(fly.w))
            .h(px(fly.h))
            .rounded(radius)
            .overflow_hidden()
            .border_1()
            .border_color(gpui::white().opacity(0.14 * t))
            .opacity(fade)
            .cursor_pointer()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |pill, event: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    if let Some(shelf) = pill.shelf_mut(id) {
                        shelf.press = Some((f32::from(event.position.x), f32::from(event.position.y)));
                    }
                }),
            )
            .on_click(cx.listener(move |pill, _: &ClickEvent, _, _| pill.shelf_open(id)))
            .child(img(shelf.image.clone()).size_full().object_fit(gpui::ObjectFit::Cover));

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
                            .on_click(cx.listener(move |pill, _: &ClickEvent, _, cx| pill.remove_shelf(id, cx)))
                            .hover_bg(("shelf-dismiss-fx", id as usize), chip_bg, gpui::black().opacity(0.8)),
                    )
                    .child(
                        dot("shelf-folder", "icons/folder.svg")
                            .tooltip(crate::hover::tip("Abrir la carpeta"))
                            .right(px(6.))
                            .top(px(6.))
                            .on_click(cx.listener(move |pill, _: &ClickEvent, _, _| pill.shelf_folder(id)))
                            .hover_bg(("shelf-folder-fx", id as usize), chip_bg, gpui::black().opacity(0.8)),
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
                                    .on_click(cx.listener(move |pill, _: &ClickEvent, _, cx| pill.shelf_copy(id, cx)))
                                    .hover_bg(("shelf-copy-fx", id as usize), chip_bg, gpui::black().opacity(0.8)),
                            )
                            .child(
                                action("shelf-draw", "icons/pencil.svg", "Dibujar")
                                    .on_click(cx.listener(move |pill, _: &ClickEvent, window, cx| {
                                        pill.shelf_draw(id, window, cx)
                                    }))
                                    .hover_bg(("shelf-draw-fx", id as usize), chip_bg, gpui::black().opacity(0.8)),
                            )
                            .child(
                                action("shelf-text", "icons/scan-text.svg", "Texto")
                                    .on_click(cx.listener(move |pill, _: &ClickEvent, _, cx| pill.shelf_text(id, cx)))
                                    .hover_bg(("shelf-text-fx", id as usize), chip_bg, gpui::black().opacity(0.8)),
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
        // La ayuda, a la izquierda de la foto: arriba la taparía la de encima.
        let tip = veil.then(|| {
            div()
                .absolute()
                .left(px(thumb.x - 178.0))
                .top(px(thumb.y + thumb.h / 2.0 - 10.0))
                .w(px(170.))
                .flex()
                .justify_end()
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
        div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .child(shadow)
            .child(photo)
            .when(t >= 1.0 && shelf.popped.is_none(), |el| el.child(overlay))
            .children(tip)
            .into_any_element()
    }
}
