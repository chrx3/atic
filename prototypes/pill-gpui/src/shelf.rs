//! El estante: las capturas recientes, flotando abajo a la derecha, como en
//! Atic (`ShelfSurface.svelte`, `capture_shelf.rs`).
//!
//! La captura vuela desde donde se tomó hasta su lugar. Las nuevas entran
//! abajo y empujan a las anteriores hacia arriba. Con el cursor encima de una
//! aparecen sus opciones: descartar, carpeta, Copiar, Dibujar y Texto. Clic en
//! la foto la abre; arrastrarla la suelta en otra app. Las grabaciones de
//! pantalla llegan igual, con su primer cuadro y solo Copiar (el archivo). Cada una se
//! achica en dos pasos durante sus 20 s y al vencer hace «pop»; con el cursor
//! encima vuelve a su tamaño y la cuenta se pausa.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use atic_capture::Frame;
use gpui::{
    div, img, point, prelude::*, px, rgb, svg, AnyElement, BoxShadow, ClickEvent, Context, Hsla,
    MouseButton, MouseDownEvent, RenderImage, SharedString,
};

use crate::anim::{ease_island, ease_smooth_out, lerp, segment, Tween};
use crate::capture::Saved;
use crate::geometry::Rect;
use crate::hover::HoverExt;
use crate::i18n::t;

const THUMB_W: f32 = 192.0;
const THUMB_H: f32 = 120.0;
/// El lado mayor de la foto que se dibuja: alcanza para el vuelo desde la
/// captura sin cargar la pantalla entera en la GPU.
const PHOTO_MAX: u32 = 768;
/// Entre una foto y la de arriba.
const GAP: f32 = 10.0;
const MARGIN: f32 = 16.0;
const FLY_MS: f32 = 340.0;
/// Lo que tarda una foto en subir cuando entra otra o se va una.
const SLIDE: Duration = Duration::from_millis(320);
const LIFETIME: Duration = Duration::from_secs(20);
/// Hasta dónde se achica al acercarse el vencimiento.
const END_SCALE: f32 = 0.6;
/// Se achica por fases: entera el primer tercio de su vida, a `MID_SCALE` el
/// segundo y a `END_SCALE` el último, cada paso en `AGE_STEP`. Entre paso y
/// paso no hay nada que dibujar (antes se achicaba de a poco los 20 s y la
/// pill redibujaba ~20 veces por segundo todo ese rato).
const MID_SCALE: f32 = 0.8;
const AGE_STEP: Duration = Duration::from_millis(300);
/// El «pop» del final: crece un poco y se desvanece.
const POP: Duration = Duration::from_millis(200);
const POP_GROW: f32 = 0.18;
/// Mover esto con el botón apretado sobre la miniatura es arrastrarla.
const DRAG_START: f32 = 6.0;
/// Lo que se ve un aviso («Texto copiado») antes de volver a los botones.
const NOTE_FOR: Duration = Duration::from_millis(1600);

pub struct Shelf {
    /// Identifica la foto aunque cambie de lugar en la pila.
    id: u64,
    /// La foto achicada que se dibuja. Con la captura entera (`img` de un
    /// PNG), GPUI la decodificaba y la subía a la GPU a tamaño de pantalla, y
    /// la guardaba en su caché para siempre: ~40 MB por captura.
    photo: Arc<RenderImage>,
    /// La captura, comprimida: Copiar, Dibujar y Texto la decodifican al
    /// usarse. Guardarla en BGRA los 20 s que vive eran ~20 MB por foto.
    png: Arc<Vec<u8>>,
    path: PathBuf,
    /// Dónde se tomó, en la ventana: de ahí sale volando.
    from: Rect,
    born: Instant,
    /// Su lugar en la pila (0 abajo), animado al subir.
    slot: Tween,
    /// Con el cursor encima vuelve a su tamaño (0 achicada, 1 entera).
    grow: Tween,
    /// El tamaño de su fase de vida (1, `MID_SCALE` o `END_SCALE`).
    age: Tween,
    /// Venció: desde cuándo hace «pop».
    popped: Option<Instant>,
    /// Tiempo de vida que queda; solo corre sin el cursor encima.
    left: Duration,
    /// Cuánto vive en total (`capture_shelf_timeout_seconds`); `None` = hasta
    /// cerrarla.
    lifetime: Option<Duration>,
    /// A la izquierda de la pantalla (`capture_shelf_side`).
    on_left: bool,
    last_tick: Instant,
    hovered: bool,
    press: Option<(f32, f32)>,
    /// El botón izquierdo en el sondeo anterior: para ver cuándo se aprieta.
    was_down: bool,
    note: Option<(SharedString, Instant)>,
    busy: bool,
    /// Una grabación de pantalla: la foto es su primer cuadro; Copiar copia
    /// el archivo y no hay Dibujar ni Texto.
    video: bool,
}

/// El tamaño de la fase en que va, por tercios de su vida: entera,
/// `MID_SCALE` y `END_SCALE`. Sin vencimiento, siempre entera.
fn age_scale(left: Duration, lifetime: Option<Duration>) -> f32 {
    let Some(total) = lifetime.filter(|total| !total.is_zero()) else {
        return 1.0;
    };
    let life = left.as_secs_f32() / total.as_secs_f32();
    if life > 2.0 / 3.0 {
        1.0
    } else if life > 1.0 / 3.0 {
        MID_SCALE
    } else {
        END_SCALE
    }
}

impl Drop for Shelf {
    fn drop(&mut self) {
        crate::release::image(self.photo.clone());
    }
}

/// La captura achicada a `PHOTO_MAX`, en BGRA como la pide GPUI.
fn photo(frame: &Frame) -> Arc<RenderImage> {
    let (width, height) = (frame.width().max(1), frame.height().max(1));
    let fit = (PHOTO_MAX as f32 / width.max(height) as f32).min(1.0);
    let (w, h) = (((width as f32 * fit).round() as u32).max(1), ((height as f32 * fit).round() as u32).max(1));
    let small = image::ImageBuffer::<image::Rgba<u8>, &[u8]>::from_raw(width, height, &frame.bgra[..])
        .map(|full| image::imageops::thumbnail(&full, w, h))
        .unwrap_or_else(|| image::RgbaImage::new(1, 1));
    Arc::new(RenderImage::new([image::Frame::new(small)]))
}

impl crate::Pill {
    pub(crate) fn show_shelf(&mut self, saved: Saved, from: Rect) {
        let now = Instant::now();
        crate::debug(|| format!("estante: {}×{} desde {from:?}", saved.width, saved.height));
        let cfg = atic_core::AppDirs::new().ok().map(|dirs| atic_core::Config::load(&dirs.config_path()));
        let lifetime = match cfg.as_ref().map(|cfg| cfg.capture_shelf_timeout_seconds) {
            Some(0) => None,
            Some(seconds) => Some(Duration::from_secs(u64::from(seconds))),
            None => Some(LIFETIME),
        };
        let on_left = cfg.is_some_and(|cfg| cfg.capture_shelf_side == "left");
        self.shelf_seq += 1;
        let video = saved.path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("mp4"));
        self.shelves.insert(
            0,
            Shelf {
                id: self.shelf_seq,
                photo: photo(&saved.frame),
                png: Arc::new(saved.png),
                path: saved.path,
                from,
                born: now,
                slot: Tween::new(0.0, SLIDE, ease_island),
                grow: Tween::new(0.0, Duration::from_millis(200), ease_smooth_out),
                age: Tween::new(1.0, AGE_STEP, ease_smooth_out),
                popped: None,
                left: lifetime.unwrap_or(LIFETIME),
                lifetime,
                on_left,
                last_tick: now,
                hovered: false,
                press: None,
                was_down: false,
                note: None,
                busy: false,
                video,
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
    fn thumb_rect(&self, slot: f32, on_left: bool) -> Rect {
        Rect::new(
            if on_left { self.work.x + MARGIN } else { self.work.right() - THUMB_W - MARGIN },
            self.work.bottom() - THUMB_H - MARGIN - slot * (THUMB_H + GAP),
            THUMB_W,
            THUMB_H,
        )
    }

    /// Cuánto mide ahora: se achica con el tiempo, vuelve con el cursor
    /// encima y al vencer crece un poco mientras se desvanece.
    fn photo_scale(shelf: &Shelf, now: Instant) -> f32 {
        let scale = lerp(shelf.age.value(now), 1.0, shelf.grow.value(now));
        match shelf.popped {
            Some(at) => scale * (1.0 + POP_GROW * Self::pop_progress(at, now)),
            None => scale,
        }
    }


    fn pop_progress(at: Instant, now: Instant) -> f32 {
        (now.duration_since(at).as_secs_f32() / POP.as_secs_f32()).clamp(0.0, 1.0)
    }

    /// La foto en su lugar, a su tamaño de ahora: pegada al borde de su lado
    /// y centrada en el alto de su lugar.
    fn photo_rect(&self, shelf: &Shelf, now: Instant) -> Rect {
        let slot = self.thumb_rect(shelf.slot.value(now), shelf.on_left);
        let scale = Self::photo_scale(shelf, now);
        let (w, h) = (slot.w * scale, slot.h * scale);
        let x = if shelf.on_left { slot.x } else { slot.right() - w };
        Rect::new(x, slot.y + (slot.h - h) / 2.0, w, h)
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
            if !over && shelf.press.is_none() && !shelf.busy && shelf.lifetime.is_some() {
                shelf.left = shelf.left.saturating_sub(elapsed);
            }
            let age = age_scale(shelf.left, shelf.lifetime);
            if shelf.age.target() != age {
                shelf.age.set(age, now);
                cx.notify();
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
            // El aviso se va y vuelven los botones (antes quedaba para siempre).
            if shelf.note.is_some() && Self::shelf_note_shown(shelf).is_none() {
                shelf.note = None;
                cx.notify();
            }
        }
        if !expired.is_empty() {
            self.shelves.retain(|shelf| !expired.contains(&shelf.id));
            self.restack(now);
            cx.notify();
        }
        over_any
    }

    /// Lo que pide cuadros: el vuelo, la subida, el crecer con el cursor, los
    /// pasos del achique y el «pop». Entre medio, la foto está quieta.
    pub(crate) fn shelf_animating(&self, now: Instant) -> bool {
        self.shelves.iter().any(|shelf| {
            now.duration_since(shelf.born).as_secs_f32() * 1000.0 < FLY_MS
                || shelf.slot.is_running(now)
                || shelf.grow.is_running(now)
                || shelf.age.is_running(now)
                || shelf.popped.is_some()
        })
    }

    /// La pill tiene que sondear rápido: algo de la bandeja se mueve o el
    /// cursor está encima (los botones, arrastrarla). Si no, la cuenta
    /// regresiva sigue igual con el sondeo en calma.
    pub(crate) fn shelf_needs_frames(&self, now: Instant) -> bool {
        self.shelf_animating(now) || self.shelves.iter().any(|shelf| shelf.hovered || shelf.press.is_some())
    }

    /// El aviso de la foto, mientras dura: «Leyendo…» hasta que termina, los
    /// demás `NOTE_FOR`. Después vuelven los botones.
    fn shelf_note_shown(shelf: &Shelf) -> Option<&SharedString> {
        shelf
            .note
            .as_ref()
            .filter(|(_, at)| shelf.busy || at.elapsed() < NOTE_FOR)
            .map(|(note, _)| note)
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
        let png = shelf.png.clone();
        let video = shelf.video.then(|| shelf.path.to_string_lossy().into_owned());
        // Decodificar una pantalla entera toma su rato: va en un hilo.
        cx.spawn(async move |this, cx| {
            let copied = cx
                .background_spawn(async move {
                    match video {
                        Some(path) => crate::clip_image::write_files(&[path]),
                        None => {
                            let frame = Frame::from_png(0, 0, &png).map_err(|error| error.to_string())?;
                            crate::clip_image::write(frame.width(), frame.height(), &frame.bgra, &png)
                        }
                    }
                })
                .await;
            let _ = this.update(cx, |pill, cx| {
                match copied {
                    Ok(()) => {
                        // Copiar es lo que más se hace: copia y se va.
                        pill.remove_shelf(id, cx);
                        println!("estante: copiada");
                    }
                    Err(error) => pill.shelf_note(id, &crate::i18n::tf("pill.shelf.copyFailed", &[("error", &error)]), cx),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn shelf_draw(&mut self, id: u64, window: &mut gpui::Window, cx: &mut Context<Self>) {
        let Some(index) = self.shelves.iter().position(|shelf| shelf.id == id) else {
            return;
        };
        let shelf = self.shelves.remove(index);
        self.restack(Instant::now());
        // Si salió de la pizarra, vuelve limpia y con sus figuras editables.
        if self.reopen_edit(&shelf.path, window, cx) {
            return;
        }
        let previous = crate::paste::foreground_target();
        match Frame::from_png(0, 0, &shelf.png) {
            Ok(frame) => self.open_board_centered(frame, previous, window, cx),
            Err(error) => eprintln!("estante: no se pudo abrir la captura: {error}"),
        }
    }

    fn shelf_text(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(shelf) = self.shelf_mut(id) else {
            return;
        };
        if shelf.busy {
            return;
        }
        shelf.busy = true;
        shelf.note = Some((t("shelf.reading").into(), Instant::now()));
        let png = shelf.png.clone();
        cx.spawn(async move |this, cx| {
            let text = cx
                .background_spawn(async move {
                    let frame = Frame::from_png(0, 0, &png).map_err(|error| error.to_string())?;
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
                        pill.shelf_note(id, t("toast.textCopied"), cx);
                    }
                    Ok(_) => pill.shelf_note(id, t("pill.shelf.noText"), cx),
                    Err(error) => {
                        eprintln!("estante: OCR: {error}");
                        pill.shelf_note(id, t("pill.clipboard.readFailed"), cx);
                    }
                }
            });
        })
        .detach();
        cx.notify();
    }

    fn shelf_open(&mut self, id: u64) {
        if let Some(shelf) = self.shelf_mut(id) {
            let _ = crate::paths::open(&shelf.path);
        }
    }

    fn shelf_folder(&mut self, id: u64) {
        if let Some(shelf) = self.shelf_mut(id) {
            let _ = crate::paths::reveal(&shelf.path);
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
                .font_family(crate::fonts::UI)
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
        let note = Self::shelf_note_shown(shelf);
        let veil = shelf.hovered && note.is_none() && t >= 1.0;

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
            .child(img(shelf.photo.clone()).size_full().object_fit(gpui::ObjectFit::Cover))
            .when(shelf.video, |el| {
                el.child(
                    div()
                        .absolute()
                        .left(px(8.))
                        .bottom(px(8.))
                        .size(px(24.))
                        .rounded_full()
                        .bg(gpui::black().opacity(0.6))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(svg().path("icons/play.svg").size(px(11.)).text_color(gpui::white())),
                )
            });
        // Un video solo tiene Copiar al centro.
        let actions = if shelf.video { 1.0 } else { 3.0 };

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
                            .tooltip(crate::hover::tip(crate::i18n::t("shelf.dismiss")))
                            .left(px(6.))
                            .top(px(6.))
                            .on_click(cx.listener(move |pill, _: &ClickEvent, _, cx| pill.remove_shelf(id, cx)))
                            .hover_bg(("shelf-dismiss-fx", id as usize), chip_bg, gpui::black().opacity(0.8)),
                    )
                    .child(
                        dot("shelf-folder", "icons/folder.svg")
                            .tooltip(crate::hover::tip(crate::i18n::t("pill.shelf.openFolder")))
                            .right(px(6.))
                            .top(px(6.))
                            .on_click(cx.listener(move |pill, _: &ClickEvent, _, _| pill.shelf_folder(id)))
                            .hover_bg(("shelf-folder-fx", id as usize), chip_bg, gpui::black().opacity(0.8)),
                    )
                    .child(
                        // `.shelf-center`: una columna al centro de la foto.
                        div()
                            .absolute()
                            .top(px((thumb.h - (26.0 * actions + 5.0 * (actions - 1.0))) / 2.0))
                            .left(px((thumb.w - 100.0) / 2.0))
                            .w(px(100.))
                            .flex()
                            .flex_col()
                            .gap(px(5.))
                            .child(
                                action("shelf-copy", "icons/copy.svg", crate::i18n::t("shelf.copy"))
                                    .on_click(cx.listener(move |pill, _: &ClickEvent, _, cx| pill.shelf_copy(id, cx)))
                                    .hover_bg(("shelf-copy-fx", id as usize), chip_bg, gpui::black().opacity(0.8)),
                            )
                            .when(!shelf.video, |el| {
                                el.child(
                                    action("shelf-draw", "icons/pencil.svg", crate::i18n::t("shelf.draw"))
                                        .on_click(cx.listener(move |pill, _: &ClickEvent, window, cx| {
                                            pill.shelf_draw(id, window, cx)
                                        }))
                                        .hover_bg(("shelf-draw-fx", id as usize), chip_bg, gpui::black().opacity(0.8)),
                                )
                                .child(
                                    action("shelf-text", "icons/scan-text.svg", crate::i18n::t("shelf.text"))
                                        .on_click(cx.listener(move |pill, _: &ClickEvent, _, cx| pill.shelf_text(id, cx)))
                                        .hover_bg(("shelf-text-fx", id as usize), chip_bg, gpui::black().opacity(0.8)),
                                )
                            }),
                    )
            })
            .when_some(note, |el, note| {
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
        // La ayuda, al lado de la foto que da a la pantalla: arriba la taparía
        // la de encima.
        let on_left = shelf.on_left;
        let tip = veil.then(|| {
            div()
                .absolute()
                .left(px(if on_left { thumb.right() + 8.0 } else { thumb.x - 178.0 }))
                .top(px(thumb.y + thumb.h / 2.0 - 10.0))
                .w(px(170.))
                .flex()
                .when(on_left, |el| el.justify_start())
                .when(!on_left, |el| el.justify_end())
                .child(
                    div()
                        .px(px(8.))
                        .py(px(3.))
                        .rounded(px(8.))
                        .bg(gpui::black().opacity(0.78))
                        .text_size(px(10.))
                        .text_color(text)
                        .child(crate::i18n::t("pill.shelf.hint")),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn se_achica_en_dos_pasos_por_tercios() {
        let total = Some(Duration::from_secs(21));
        assert_eq!(age_scale(Duration::from_secs(21), total), 1.0);
        assert_eq!(age_scale(Duration::from_secs(15), total), 1.0);
        assert_eq!(age_scale(Duration::from_secs(13), total), MID_SCALE);
        assert_eq!(age_scale(Duration::from_secs(8), total), MID_SCALE);
        assert_eq!(age_scale(Duration::from_secs(6), total), END_SCALE);
        assert_eq!(age_scale(Duration::ZERO, total), END_SCALE);
    }

    #[test]
    fn sin_vencimiento_no_se_achica() {
        assert_eq!(age_scale(Duration::ZERO, None), 1.0);
    }
}
