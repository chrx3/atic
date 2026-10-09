//! Capturas: la mira sobre la pantalla congelada.
//!
//! En Atic la mira es un WebView: congelar, codificar JPEG, escribirlo a disco
//! y que el WebView lo cargue y decodifique, con un WebView precalentado para
//! que no tarde segundos en frío. Aquí el frame BGRA de `atic-capture` va
//! directo a una textura y se dibuja en la ventana del overlay que ya existe:
//! sin JPEG, sin disco, sin ventana nueva.
//!
//! Con varias pantallas la ventana se estira a cubrirlas todas, aunque tengan
//! escalas distintas: todo se calcula en píxeles físicos con la escala que la
//! ventana tenga al mostrar la mira (Windows puede cambiársela al estirarla).
//! La ventana elegida se recorta del frame congelado (Atic la re-renderiza con
//! `PrintWindow` para las tapadas).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use atic_capture::{monitors, Frame, Rect as PhysRect};
use atic_core::{AppDirs, Config};
use gpui::{
    actions, canvas, div, fill, img, outline, point, prelude::*, px, rgb, size, App, Bounds,
    BorderStyle, Context, CursorStyle, EventEmitter, FocusHandle, Focusable, Hsla, KeyBinding,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, RenderImage, SharedString, Window,
};

use crate::geometry::Rect;

/// Lo que mide la mira para distinguir un clic de un arrastre.
const DRAG_MIN: f32 = 4.0;

pub struct Candidate {
    /// Lógico, en coordenadas del overlay.
    pub rect: Rect,
    pub title: SharedString,
}

/// La pantalla congelada, lista para dibujar y para recortar.
pub struct Frozen {
    pub frame: Frame,
    pub image: Arc<RenderImage>,
    pub scale: f32,
    pub windows: Vec<Candidate>,
    pub took: std::time::Duration,
    /// Dónde cae la esquina del monitor en la ventana del overlay, en lógico.
    /// El área cliente no empieza justo en (0, 0) de la pantalla: sin esto la
    /// foto congelada queda corrida un píxel y se ve saltar todo.
    pub offset: (f32, f32),
}

impl Frozen {
    /// Pasa lo lógico a otra escala de la ventana (las ventanas candidatas;
    /// la foto y los recortes usan `scale`, que también cambia).
    pub fn rescale(&mut self, scale: f32) {
        if (scale - self.scale).abs() < f32::EPSILON || scale <= 0.0 {
            return;
        }
        let ratio = self.scale / scale;
        for candidate in &mut self.windows {
            let r = candidate.rect;
            candidate.rect = Rect::new(r.x * ratio, r.y * ratio, r.w * ratio, r.h * ratio);
        }
        self.scale = scale;
    }

    /// Un punto de la ventana, en coordenadas de la foto.
    pub fn local(&self, p: gpui::Point<gpui::Pixels>) -> (f32, f32) {
        (f32::from(p.x) - self.offset.0, f32::from(p.y) - self.offset.1)
    }
}

/// Congela el monitor principal. Corre en un hilo de fondo.
/// Congela `area` (píxeles físicos de pantalla): un monitor o, con todos a la
/// misma escala, el escritorio entero.
pub fn freeze(scale: f32, area: PhysRect) -> Result<Frozen, String> {
    let started = Instant::now();
    let all = monitors::enumerate();
    let frame = atic_capture::engine::capture_rect(area, include_cursor()).map_err(|error| error.to_string())?;
    let origin = (area.x, area.y);
    let to_logical = |rect: &PhysRect| {
        Rect::new(
            (rect.x - origin.0) as f32 / scale,
            (rect.y - origin.1) as f32 / scale,
            rect.width as f32 / scale,
            rect.height as f32 / scale,
        )
    };
    // Del frente hacia atrás: la primera que contiene el cursor es la visible.
    let windows = atic_capture::windows::enumerate_candidates(std::process::id(), &all)
        .into_iter()
        .filter_map(|candidate| {
            let visible = candidate.visual_bounds.intersection(&area)?;
            Some(Candidate {
                rect: to_logical(&visible),
                title: candidate.title.into(),
            })
        })
        .collect();
    let image = image_of(&frame).ok_or("frame con tamaño inválido")?;
    Ok(Frozen {
        frame,
        image,
        scale,
        windows,
        took: started.elapsed(),
        offset: (0.0, 0.0),
    })
}

/// GPUI dibuja BGRA tal cual: el buffer del frame sirve sin convertir.
fn image_of(frame: &Frame) -> Option<Arc<RenderImage>> {
    let buffer = image::RgbaImage::from_raw(frame.width(), frame.height(), frame.bgra.clone())?;
    Some(Arc::new(RenderImage::new([image::Frame::new(buffer)])))
}

/// Qué se captura si se suelta ahora.
#[derive(Clone, Debug, PartialEq)]
pub enum Pick {
    Region(Rect),
    Window(Rect, SharedString),
    Screen,
}

pub struct Session {
    pub frozen: Frozen,
    /// Donde se apretó el botón, mientras siga apretado.
    pub press: Option<(f32, f32)>,
    pub cursor: (f32, f32),
    /// La app que tenía el foco: la mira se lo quita para recibir Esc.
    pub previous: Option<crate::paste::Target>,
    /// En vivo, como en Mac: la mira va sobre el escritorio real y la foto se
    /// toma al soltar. Congelada, como Atic: se elige sobre la foto inicial.
    pub live: bool,
    /// D: al soltar, la región se queda y se abre la pizarra encima en vez de
    /// copiar al tiro (como Flameshot). Shift al soltar hace lo mismo.
    pub draw: bool,
    /// La foto congelada muestra la pill tal como estaba (se abrió con el
    /// atajo). P la vuelve a congelar sin ella.
    pub pill_shown: bool,
    /// Se mantuvo el atajo: lo elegido se graba en vez de fotografiarse
    /// (`screen_recording.rs`).
    pub record: bool,
    /// La foto con la pill mientras se ve la que no la tiene, para volver a ella.
    with_pill: Option<(Frame, Arc<RenderImage>)>,
    /// En vivo, los píxeles bajo el cursor para la lupa, recién leídos.
    loupe: Option<Frame>,
    /// Cada monitor, en coordenadas de la foto: la ayuda va en el del cursor.
    screens: Vec<Rect>,
}

/// El modo con que abre la mira: el último que se usó.
fn mode_file() -> Option<PathBuf> {
    crate::paths::file("capture-mode.txt")
}

/// `capture_include_cursor`: el puntero sale en la foto.
fn include_cursor() -> bool {
    AppDirs::new().is_ok_and(|dirs| Config::load(&dirs.config_path()).capture_include_cursor)
}

pub fn remembered_live() -> bool {
    mode_file()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .is_some_and(|mode| mode.trim() == "live")
}

fn remember_live(live: bool) {
    if let Some(path) = mode_file() {
        let _ = std::fs::write(path, if live { "live" } else { "frozen" });
    }
}

impl Session {
    pub fn new(frozen: Frozen, cursor: (f32, f32), previous: Option<crate::paste::Target>) -> Self {
        Self {
            frozen,
            press: None,
            cursor,
            previous,
            live: false,
            draw: false,
            pill_shown: false,
            record: false,
            with_pill: None,
            loupe: None,
            screens: Vec::new(),
        }
        .with_screens()
    }

    fn with_screens(mut self) -> Self {
        let (area, scale) = (self.frozen.frame.bounds, self.frozen.scale);
        self.screens = monitors::enumerate()
            .iter()
            .map(|monitor| {
                let m = monitor.bounds;
                Rect::new(
                    (m.x - area.x) as f32 / scale,
                    (m.y - area.y) as f32 / scale,
                    m.width as f32 / scale,
                    m.height as f32 / scale,
                )
            })
            .collect();
        self
    }

    /// El monitor bajo el cursor (o toda la foto si no se sabe).
    fn cursor_screen(&self) -> Rect {
        self.screens
            .iter()
            .copied()
            .find(|screen| screen.contains(self.cursor, 0.0))
            .unwrap_or_else(|| self.screen())
    }

    /// Hay una foto con la pill para alternar. En vivo también: P congela
    /// con la foto del arranque, la que muestra la pill y el notch abierto.
    pub fn can_toggle_pill(&self) -> bool {
        self.pill_shown || self.with_pill.is_some()
    }

    /// Congela de nuevo la pantalla: el overlay ya está excluido de las
    /// capturas, así que sale sin la pill ni la mira.
    fn refreeze(&mut self) -> Result<(), String> {
        let started = Instant::now();
        let frame = atic_capture::engine::capture_rect(self.frozen.frame.bounds, include_cursor())
            .map_err(|error| error.to_string())?;
        let image = image_of(&frame).ok_or("frame con tamaño inválido")?;
        self.frozen.image = image;
        self.frozen.frame = frame;
        self.frozen.took = started.elapsed();
        Ok(())
    }

    /// En vivo: relee los píxeles alrededor del cursor para la lupa. Es un
    /// `BitBlt` de 13×13: menos de un milisegundo.
    pub fn refresh_loupe(&mut self) {
        if !self.live {
            return;
        }
        let pixel = self.pixel_at(self.cursor);
        let half = LOUPE_PIXELS / 2;
        let bounds = self.frozen.frame.bounds;
        let rect = PhysRect::new(
            bounds.x + pixel.0 - half,
            bounds.y + pixel.1 - half,
            LOUPE_PIXELS as u32,
            LOUPE_PIXELS as u32,
        );
        // Sin `CAPTUREBLT`: con él, Windows sincroniza con el compositor en
        // cada lectura. El overlay no sale igual (está excluido de capturas).
        self.loupe = atic_capture::engine::capture_rect_without_layered(rect).ok();
    }

    /// Tamaño lógico de la pantalla congelada.
    pub fn screen(&self) -> Rect {
        let scale = self.frozen.scale;
        Rect::new(
            0.0,
            0.0,
            self.frozen.frame.width() as f32 / scale,
            self.frozen.frame.height() as f32 / scale,
        )
    }

    pub fn pick(&self) -> Pick {
        if let Some(start) = self.press {
            let (x0, y0) = (start.0.min(self.cursor.0), start.1.min(self.cursor.1));
            let (x1, y1) = (start.0.max(self.cursor.0), start.1.max(self.cursor.1));
            if x1 - x0 >= DRAG_MIN || y1 - y0 >= DRAG_MIN {
                return Pick::Region(Rect::new(x0, y0, x1 - x0, y1 - y0));
            }
        }
        self.frozen
            .windows
            .iter()
            .find(|window| window.rect.contains(self.cursor, 0.0))
            .map(|window| Pick::Window(window.rect, window.title.clone()))
            .unwrap_or(Pick::Screen)
    }

    /// El rectángulo de la elección, lógico.
    pub fn pick_rect(&self, pick: &Pick) -> Rect {
        match pick {
            Pick::Region(rect) | Pick::Window(rect, _) => *rect,
            Pick::Screen => self.screen(),
        }
    }

    /// Lógico → físico del escritorio virtual, para recortar el frame.
    pub fn physical(&self, rect: &Rect) -> PhysRect {
        let scale = self.frozen.scale;
        let bounds = self.frozen.frame.bounds;
        let left = bounds.x + (rect.x * scale).round() as i32;
        let top = bounds.y + (rect.y * scale).round() as i32;
        let right = bounds.x + (rect.right() * scale).round() as i32;
        let bottom = bounds.y + (rect.bottom() * scale).round() as i32;
        PhysRect::from_ltrb(left, top, right, bottom)
    }

    /// Píxel físico bajo un punto lógico, en coordenadas del frame.
    pub fn pixel_at(&self, (x, y): (f32, f32)) -> (i32, i32) {
        let scale = self.frozen.scale;
        ((x * scale).floor() as i32, (y * scale).floor() as i32)
    }

    pub fn color_at(&self, pixel: (i32, i32)) -> Option<[u8; 4]> {
        if self.live {
            // La lupa en vivo está centrada en el cursor.
            let center = self.pixel_at(self.cursor);
            let half = LOUPE_PIXELS / 2;
            let loupe = self.loupe.as_ref()?;
            return loupe.pixel_rgba(pixel.0 - center.0 + half, pixel.1 - center.1 + half);
        }
        self.frozen.frame.pixel_rgba(pixel.0, pixel.1)
    }
}

/// El resultado ya guardado y copiado.
pub struct Saved {
    pub path: PathBuf,
    pub png: Vec<u8>,
    pub width: u32,
    pub height: u32,
    /// El recorte en BGRA: para volver a copiarlo, leerle el texto o dibujar.
    pub frame: Frame,
}

fn captures_dir() -> Option<PathBuf> {
    crate::paths::captures_dir()
}

/// Recorta, guarda el PNG y lo deja en el portapapeles. Corre en un hilo de
/// fondo: codificar PNG de una pantalla 4K toma cientos de ms.
pub fn save(frame: &Frame, region: PhysRect) -> Result<Saved, String> {
    let cropped = frame.crop(region).ok_or("la selección quedó fuera de la pantalla")?;
    let png = cropped.to_png().map_err(|error| error.to_string())?;
    let dir = captures_dir().ok_or("sin LOCALAPPDATA")?;
    std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    let path = dir.join(atic_capture::naming::unique_capture_filename(&dir));
    std::fs::write(&path, &png).map_err(|error| error.to_string())?;
    crate::clip_image::write(cropped.width(), cropped.height(), &cropped.bgra, &png)?;
    Ok(Saved {
        path,
        width: cropped.width(),
        height: cropped.height(),
        png,
        frame: cropped,
    })
}

// --- La mira --------------------------------------------------------------

actions!(capture, [Cancel, WholeScreen, ToggleLive, ToggleDraw, TogglePill]);

const KEY_CONTEXT: &str = "Capture";

pub fn bind_keys(cx: &mut App) {
    let context = Some(KEY_CONTEXT);
    cx.bind_keys([
        KeyBinding::new("escape", Cancel, context),
        KeyBinding::new("enter", WholeScreen, context),
        KeyBinding::new("space", WholeScreen, context),
        KeyBinding::new("f", ToggleLive, context),
        KeyBinding::new("d", ToggleDraw, context),
        KeyBinding::new("p", TogglePill, context),
    ]);
}

/// Lado de la lupa en píxeles físicos: impar, para que haya un centro.
const LOUPE_PIXELS: i32 = 13;
const LOUPE_CELL: f32 = 9.0;
const LOUPE_OFFSET: f32 = 22.0;
const DIM: f32 = 0.42;
/// El borde de lo elegido para grabar: el rojo de grabar de Reuniones.
const RECORD_EDGE: u32 = crate::meetings::RECORD_RED;

pub enum CaptureEvent {
    /// Lo elegido, en físico, con el frame para recortarlo fuera de la UI.
    /// `draw`: abrir la pizarra sobre la región antes de copiar.
    Chosen {
        frame: Frame,
        region: PhysRect,
        draw: bool,
    },
    /// Grabar lo elegido, en físico.
    Record(PhysRect),
    Cancelled,
}

pub struct CaptureView {
    pub session: Session,
    focus: FocusHandle,
    meter: crate::meter::Meter,
}

impl EventEmitter<CaptureEvent> for CaptureView {}

impl Focusable for CaptureView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl CaptureView {
    pub fn new(session: Session, cx: &mut Context<Self>) -> Self {
        Self {
            session,
            focus: cx.focus_handle(),
            meter: crate::meter::Meter::new("mira"),
        }
    }

    fn choose(&mut self, pick: Pick, draw: bool, cx: &mut Context<Self>) {
        let rect = self.session.pick_rect(&pick);
        let region = self.session.physical(&rect);
        if self.session.record {
            cx.emit(CaptureEvent::Record(region));
            return;
        }
        // En vivo la foto es de ahora: el overlay está excluido de las
        // capturas, así que no hace falta esconderlo antes.
        let frame = if self.session.live {
            match atic_capture::engine::capture_rect(region, include_cursor()) {
                Ok(frame) => frame,
                Err(error) => {
                    eprintln!("captura en vivo: {error}");
                    self.session.frozen.frame.clone()
                }
            }
        } else {
            self.session.frozen.frame.clone()
        };
        cx.emit(CaptureEvent::Chosen {
            frame,
            region,
            draw,
        });
    }

    /// F: congelar lo que se ve ahora, o volver a la pantalla viva.
    fn toggle_live(&mut self, _: &ToggleLive, _: &mut Window, cx: &mut Context<Self>) {
        let session = &mut self.session;
        if session.live {
            match session.refreeze() {
                Ok(()) => {
                    session.live = false;
                    // La foto nueva es de ahora y sin la pill: la de antes ya no vale.
                    session.pill_shown = false;
                    session.with_pill = None;
                }
                Err(error) => eprintln!("captura: no se pudo congelar: {error}"),
            }
        } else {
            session.live = true;
            session.refresh_loupe();
        }
        remember_live(session.live);
        cx.notify();
    }

    /// P: la foto con la pill o sin ella.
    fn toggle_pill(&mut self, _: &TogglePill, _: &mut Window, cx: &mut Context<Self>) {
        let session = &mut self.session;
        if !session.can_toggle_pill() {
            return;
        }
        if session.pill_shown {
            let with = (session.frozen.frame.clone(), session.frozen.image.clone());
            match session.refreeze() {
                Ok(()) => {
                    session.with_pill = Some(with);
                    session.pill_shown = false;
                }
                Err(error) => eprintln!("captura: no se pudo congelar sin la pill: {error}"),
            }
        } else if let Some((frame, image)) = session.with_pill.take() {
            session.frozen.frame = frame;
            session.frozen.image = image;
            session.pill_shown = true;
            // Desde en vivo queda congelada en esa foto, sin cambiar el modo
            // con que abre la próxima vez.
            session.live = false;
        }
        cx.notify();
    }

    pub fn set_record(&mut self, cx: &mut Context<Self>) {
        self.session.record = true;
        cx.notify();
    }

    fn cancel(&mut self, _: &Cancel, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(CaptureEvent::Cancelled);
    }

    fn whole_screen(&mut self, _: &WholeScreen, _: &mut Window, cx: &mut Context<Self>) {
        let draw = self.session.draw;
        self.choose(Pick::Screen, draw, cx);
    }
}

/// Lo que pinta el canvas, copiado: el canvas se dibuja después del render.
struct Paint {
    offset: (f32, f32),
    screen: Rect,
    selection: Option<Rect>,
    window_hint: bool,
    record: bool,
    cursor: (f32, f32),
    crosshair: bool,
    loupe: Vec<Hsla>,
    loupe_origin: (f32, f32),
}

fn bounds(rect: &Rect) -> Bounds<gpui::Pixels> {
    Bounds::new(point(px(rect.x), px(rect.y)), size(px(rect.w), px(rect.h)))
}

impl Paint {
    /// En coordenadas de la foto; se corren al pintar.
    fn b(&self, rect: &Rect) -> Bounds<gpui::Pixels> {
        bounds(&Rect::new(rect.x + self.offset.0, rect.y + self.offset.1, rect.w, rect.h))
    }

    fn paint(self, window: &mut Window) {
        let shade: Hsla = gpui::black().opacity(DIM);
        let s = self.screen;
        match self.selection {
            // Todo oscuro menos la elección, en cuatro bandas.
            Some(r) => {
                let bands = [
                    Rect::new(s.x, s.y, s.w, (r.y - s.y).max(0.0)),
                    Rect::new(s.x, r.bottom(), s.w, (s.bottom() - r.bottom()).max(0.0)),
                    Rect::new(s.x, r.y, (r.x - s.x).max(0.0), r.h),
                    Rect::new(r.right(), r.y, (s.right() - r.right()).max(0.0), r.h),
                ];
                for band in bands {
                    window.paint_quad(fill(self.b(&band), shade));
                }
                let edge: Hsla = if self.record {
                    rgb(RECORD_EDGE).into()
                } else if self.window_hint {
                    rgb(0x7aa2ff).into()
                } else {
                    gpui::white().opacity(0.95)
                };
                window.paint_quad(outline(self.b(&r), edge, BorderStyle::Solid));
            }
            None => window.paint_quad(fill(self.b(&s), shade)),
        }

        if self.crosshair {
            let line: Hsla = gpui::white().opacity(0.28);
            let (x, y) = self.cursor;
            window.paint_quad(fill(self.b(&Rect::new(s.x, y.floor(), s.w, 1.0)), line));
            window.paint_quad(fill(self.b(&Rect::new(x.floor(), s.y, 1.0, s.h)), line));
        }

        // La lupa: los píxeles reales alrededor del cursor, en cuadritos.
        let (lx, ly) = self.loupe_origin;
        let side = LOUPE_PIXELS as f32 * LOUPE_CELL;
        window.paint_quad(fill(
            self.b(&Rect::new(lx - 2.0, ly - 2.0, side + 4.0, side + 4.0)),
            gpui::black().opacity(0.85),
        ));
        for (i, color) in self.loupe.iter().enumerate() {
            let col = (i as i32 % LOUPE_PIXELS) as f32;
            let row = (i as i32 / LOUPE_PIXELS) as f32;
            let cell = Rect::new(lx + col * LOUPE_CELL, ly + row * LOUPE_CELL, LOUPE_CELL, LOUPE_CELL);
            window.paint_quad(fill(self.b(&cell), *color));
        }
        let center = (LOUPE_PIXELS / 2) as f32 * LOUPE_CELL;
        window.paint_quad(outline(
            self.b(&Rect::new(lx + center, ly + center, LOUPE_CELL, LOUPE_CELL)),
            gpui::white(),
            BorderStyle::Solid,
        ));
        window.paint_quad(outline(
            self.b(&Rect::new(lx, ly, side, side)),
            gpui::white().opacity(0.6),
            BorderStyle::Solid,
        ));
    }
}

/// Etiqueta oscura y redonda sobre la pantalla congelada.
fn chip(text: impl Into<SharedString>) -> gpui::Div {
    div()
        .px(px(8.))
        .py(px(3.))
        .rounded(px(8.))
        .bg(gpui::black().opacity(0.78))
        .text_color(gpui::white())
        .text_size(px(11.))
        .font_family("Segoe UI")
        .child(text.into())
}

impl Render for CaptureView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.meter.frame(Instant::now());
        // Una lectura por cuadro, no por evento: el mouse manda cientos por
        // segundo y cada `BitBlt` le cuesta al compositor.
        self.session.refresh_loupe();
        let session = &self.session;
        let screen = session.screen();
        let help = session.cursor_screen();
        let pick = session.pick();
        let selection = match &pick {
            Pick::Screen => None,
            other => Some(session.pick_rect(other)),
        };
        let cursor = session.cursor;
        let pixel = session.pixel_at(cursor);
        let half = LOUPE_PIXELS / 2;
        let loupe: Vec<Hsla> = (-half..=half)
            .flat_map(|dy| (-half..=half).map(move |dx| (dx, dy)))
            .map(|(dx, dy)| match session.color_at((pixel.0 + dx, pixel.1 + dy)) {
                Some([r, g, b, _]) => rgb(u32::from_be_bytes([0, r, g, b])).into(),
                None => gpui::black(),
            })
            .collect();
        let side = LOUPE_PIXELS as f32 * LOUPE_CELL;
        // La lupa va abajo a la derecha del cursor; si no cabe, al otro lado.
        let lx = if cursor.0 + LOUPE_OFFSET + side + 8.0 > screen.right() {
            cursor.0 - LOUPE_OFFSET - side
        } else {
            cursor.0 + LOUPE_OFFSET
        };
        let ly = if cursor.1 + LOUPE_OFFSET + side + 40.0 > screen.bottom() {
            cursor.1 - LOUPE_OFFSET - side - 26.0
        } else {
            cursor.1 + LOUPE_OFFSET
        };
        let hex = session
            .color_at(pixel)
            .map(|[r, g, b, _]| format!("#{r:02X}{g:02X}{b:02X}"))
            .unwrap_or_default();

        let offset = session.frozen.offset;
        let paint = Paint {
            offset,
            screen,
            selection,
            window_hint: matches!(pick, Pick::Window(..)),
            record: session.record,
            cursor,
            crosshair: session.press.is_none(),
            loupe,
            loupe_origin: (lx, ly),
        };

        let label = selection.map(|rect| {
            let phys = session.physical(&rect);
            let dims = format!("{} × {}", phys.width, phys.height);
            let text = match &pick {
                Pick::Window(_, title) if !title.is_empty() => {
                    let title: String = title.chars().take(48).collect();
                    format!("{title} · {dims}")
                }
                _ => dims,
            };
            let below = rect.bottom() + 28.0 < screen.bottom();
            let y = if below {
                rect.bottom() + 6.0
            } else {
                (rect.y - 26.0).max(6.0)
            };
            div()
                .absolute()
                .left(px(rect.x.max(6.0) + offset.0))
                .top(px(y + offset.1))
                .child(chip(text))
        });

        div()
            .id("capture")
            .track_focus(&self.focus)
            .key_context(KEY_CONTEXT)
            .on_action(cx.listener(Self::cancel))
            .on_action(cx.listener(Self::whole_screen))
            .on_action(cx.listener(Self::toggle_live))
            .on_action(cx.listener(Self::toggle_pill))
            .on_action(cx.listener(|view, _: &ToggleDraw, _, cx| {
                view.session.draw = !view.session.draw;
                cx.notify();
            }))
            .size_full()
            .relative()
            .cursor(CursorStyle::Crosshair)
            .on_mouse_move(cx.listener(|view, event: &MouseMoveEvent, _, cx| {
                view.session.cursor = view.session.frozen.local(event.position);
                cx.notify();
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|view, event: &MouseDownEvent, _, cx| {
                    let p = view.session.frozen.local(event.position);
                    view.session.cursor = p;
                    view.session.press = Some(p);
                    cx.stop_propagation();
                    cx.notify();
                }),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|view, event: &MouseUpEvent, _, cx| {
                    view.session.cursor = view.session.frozen.local(event.position);
                    // El soltar del clic que abrió la captura (desde la gota, que
                    // abre al presionar) no es una elección: capturaba al tiro.
                    if view.session.press.is_none() {
                        return;
                    }
                    let pick = view.session.pick();
                    view.session.press = None;
                    let draw = view.session.draw || event.modifiers.shift;
                    view.choose(pick, draw, cx);
                }),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|_, _: &MouseDownEvent, _, cx| cx.emit(CaptureEvent::Cancelled)),
            )
            .when(!session.live, |el| el.child(
                img(session.frozen.image.clone())
                    .absolute()
                    .left(px(offset.0))
                    .top(px(offset.1))
                    .w(px(screen.w))
                    .h(px(screen.h)),
            ))
            .child(
                canvas(|_, _, _| {}, move |_, _, window, _| paint.paint(window))
                    .absolute()
                    .size_full(),
            )
            .children(label)
            .child(
                div()
                    .absolute()
                    .left(px(lx + offset.0))
                    .top(px(ly + side + 6.0 + offset.1))
                    .child(chip(format!("{hex} · {}, {}", pixel.0, pixel.1))),
            )
            .child(
                div()
                    .absolute()
                    .left(px(help.x + offset.0))
                    .top(px(help.y + 10.0 + offset.1))
                    .w(px(help.w))
                    .flex()
                    .justify_center()
                    .child(chip(if session.record {
                        "● Grabar · clic en una ventana o arrastra una zona · Espacio: pantalla completa · Esc".to_string()
                    } else {
                        let mode = if session.live {
                            "En vivo · F: congelar".to_string()
                        } else {
                            format!("Congelada en {} ms · F: en vivo", session.frozen.took.as_millis())
                        };
                        let draw = if session.draw {
                            "Al soltar se abre la pizarra · D: copiar directo"
                        } else {
                            "D o Shift al soltar: dibujar antes de copiar"
                        };
                        let pill = if !session.can_toggle_pill() {
                            ""
                        } else if session.pill_shown {
                            "P: ocultar la pill · "
                        } else {
                            "P: mostrar la pill · "
                        };
                        format!("{mode} · {pill}{draw} · Espacio: pantalla completa · Esc")
                    })),
            )
    }
}

// --- En la pill ------------------------------------------------------------
//
// La mira vive en la ventana del overlay: congelar, mostrar la entidad encima
// de todo y, al elegir, recortar y guardar en otro hilo. El resultado cuelga
// del notch unos segundos con la burbuja de vista previa.

/// «Capturas» en la tira y la rueda.
pub const TOOL: usize = 5;

/// Todos los monitores juntos: una sola ventana los cubre. Con uno solo,
/// `None`. Las escalas pueden ser distintas: la ventana tiene una sola y la
/// foto se dibuja 1:1 en píxeles físicos, así que calza en todas.
fn whole_desktop() -> Option<PhysRect> {
    let all = monitors::enumerate();
    if all.len() < 2 {
        return None;
    }
    let left = all.iter().map(|m| m.bounds.x).min()?;
    let top = all.iter().map(|m| m.bounds.y).min()?;
    let right = all.iter().map(|m| m.bounds.right()).max()?;
    let bottom = all.iter().map(|m| m.bounds.bottom()).max()?;
    Some(PhysRect::new(left, top, (right - left) as u32, (bottom - top) as u32))
}

fn to_win_rect(rect: PhysRect) -> windows::Win32::Foundation::RECT {
    windows::Win32::Foundation::RECT {
        left: rect.x,
        top: rect.y,
        right: rect.right(),
        bottom: rect.bottom(),
    }
}

/// La ventana de la pill, estirada a todo el escritorio para la mira.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Span {
    /// Cubre el escritorio entero.
    Covering(PhysRect),
    /// Volviendo a su monitor: hasta que llegue no se sigue al monitor.
    Restoring(windows::Win32::Foundation::RECT),
}

impl crate::Pill {
    /// El monitor de la pill, en píxeles físicos: lo que congelan la pizarra
    /// y el flip.
    pub(crate) fn pill_area(&self) -> Option<PhysRect> {
        let monitor = self.screen.as_ref()?.0.monitor;
        Some(PhysRect::new(
            monitor.left,
            monitor.top,
            (monitor.right - monitor.left) as u32,
            (monitor.bottom - monitor.top) as u32,
        ))
    }

    /// Lo que se puede capturar: todas las pantallas si comparten escala; si
    /// no, la de la pill.
    fn capture_area(&self) -> Option<(PhysRect, bool)> {
        match whole_desktop() {
            Some(area) => Some((area, true)),
            None => self.pill_area().map(|area| (area, false)),
        }
    }

    /// La ventana vuelve a su monitor tras la mira (o la pizarra que salió de
    /// ella).
    pub(crate) fn end_span(&mut self) {
        if !matches!(self.span, Some(Span::Covering(_))) {
            return;
        }
        let home = self.screen.as_ref().map(|(screen, _, _)| *screen);
        match (self.overlay.as_ref(), home) {
            (Some(overlay), Some(home)) => {
                overlay.move_to(&home);
                self.span = Some(Span::Restoring(home.monitor));
            }
            _ => self.span = None,
        }
    }

    /// `with_pill`: la foto congelada sale con la pill tal como está (para
    /// mostrar lo que tiene), y P la quita. Desde la rueda o la tira no: la
    /// foto saldría con ellas abiertas.
    pub(crate) fn start_capture(&mut self, with_pill: bool, window: &mut Window, cx: &mut Context<Self>) {
        // Grabando la pantalla, el atajo la detiene.
        if self.screen_rec.active() {
            self.stop_screen_recording(cx);
            return;
        }
        // Con la mira abierta, el mismo atajo la cierra.
        if self.capture.is_some() {
            self.end_capture(&CaptureEvent::Cancelled, window, cx);
            return;
        }
        if self.capture_pending {
            return;
        }
        self.capture_pending = true;
        if !with_pill {
            self.hide_for_capture(cx);
        }
        let previous = crate::paste::foreground_target();
        let scale = self.scale_factor;
        let Some((area, whole)) = self.capture_area() else {
            self.capture_pending = false;
            return;
        };
        cx.spawn_in(window, async move |this, cx| {
            let frozen = cx.background_spawn(async move { freeze(scale, area) }).await;
            // Con todas las pantallas, la ventana se estira a cubrirlas recién
            // ahora (la foto ya se tomó) y la mira espera a que llegue.
            if whole && frozen.is_ok() {
                let _ = this.update(cx, |pill, _| {
                    if let Some(overlay) = pill.overlay.as_ref() {
                        overlay.cover(to_win_rect(area));
                        pill.span = Some(Span::Covering(area));
                    }
                });
                for _ in 0..40 {
                    let covered = this
                        .update(cx, |pill, _| pill.overlay.as_ref().is_some_and(|o| o.covers(&to_win_rect(area))))
                        .unwrap_or(true);
                    if covered {
                        break;
                    }
                    cx.background_executor().timer(std::time::Duration::from_millis(15)).await;
                }
            }
            let _ = this.update_in(cx, |pill, window, cx| {
                if with_pill {
                    pill.hide_for_capture(cx);
                }
                pill.show_capture(frozen, area, with_pill, previous, window, cx)
            });
        })
        .detach();
    }

    /// Recoge la pill y la saca de las capturas: la mira vive en su ventana.
    fn hide_for_capture(&mut self, cx: &mut Context<Self>) {
        let now = Instant::now();
        self.close_wheel();
        self.close_panel(false, cx);
        self.peek.set(0.0, now);
        self.strip.set(0.0, now);
        if let Some(overlay) = self.overlay.as_ref() {
            overlay.exclude_from_capture(true);
        }
    }

    fn show_capture(
        &mut self,
        frozen: Result<Frozen, String>,
        area: PhysRect,
        with_pill: bool,
        previous: Option<crate::paste::Target>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.capture_pending = false;
        let mut frozen = match frozen {
            Ok(frozen) => frozen,
            Err(error) => {
                eprintln!("captura: no se pudo congelar: {error}");
                if let Some(overlay) = self.overlay.as_ref() {
                    overlay.exclude_from_capture(false);
                }
                self.end_span();
                return;
            }
        };
        // Al estirarse sobre pantallas de otra escala, Windows le cambia la
        // suya a la ventana: lo lógico se recalcula con la que tiene ahora.
        let scale = window.scale_factor();
        frozen.rescale(scale);
        // Dónde cae el área congelada en la ventana: el monitor de la pill o,
        // estirada, todo el escritorio.
        frozen.offset = self
            .overlay
            .as_ref()
            .and_then(|overlay| overlay.to_logical(&to_win_rect(area), scale))
            .map_or((self.monitor.x, self.monitor.y), |rect| (rect.x, rect.y));
        println!(
            "captura: congelada en {} ms, {} ventanas",
            frozen.took.as_millis(),
            frozen.windows.len()
        );
        // El cursor, leído ahora: la ventana pudo haberse estirado.
        let cursor = self
            .overlay
            .as_ref()
            .and_then(|overlay| overlay.cursor(scale))
            .map(|(x, y)| (x - frozen.offset.0, y - frozen.offset.1))
            .unwrap_or_default();
        let mut session = Session::new(frozen, cursor, previous);
        session.live = remembered_live();
        session.record = std::mem::take(&mut self.screen_rec.requested);
        if with_pill && session.live {
            // En vivo la foto del arranque (con la pill) queda para P.
            session.with_pill = Some((session.frozen.frame.clone(), session.frozen.image.clone()));
        } else {
            session.pill_shown = with_pill;
        }
        session.refresh_loupe();
        let view = cx.new(|cx| CaptureView::new(session, cx));
        self.capture_events = Some(cx.subscribe_in(
            &view,
            window,
            |pill, _, event: &CaptureEvent, window, cx| pill.end_capture(event, window, cx),
        ));
        if let Some(overlay) = self.overlay.as_mut() {
            overlay.set_passthrough(false);
            overlay.set_focusable(true);
        }
        window.focus(&view.focus_handle(cx));
        self.capture = Some(view);
        cx.notify();
    }

    fn end_capture(&mut self, event: &CaptureEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(view) = self.capture.take() else {
            return;
        };
        self.capture_events = None;
        let previous = view.read(cx).session.previous;
        if let Some(overlay) = self.overlay.as_ref() {
            overlay.exclude_from_capture(false);
        }
        // Dibujar antes de copiar: la región, centrada en la pizarra sobre
        // fondo oscuro (en su lugar quedaba chica o tapada). El foco sigue en
        // el overlay; si estaba estirado, vuelve al cerrar la pizarra.
        if let CaptureEvent::Chosen {
            frame,
            region,
            draw: true,
        } = event
        {
            if let Some(cropped) = frame.crop(*region) {
                self.open_board_centered(cropped, previous, window, cx);
                return;
            }
        }
        // La pizarra no se abrió: la ventana vuelve a su monitor.
        self.end_span();
        if let Some(overlay) = self.overlay.as_mut() {
            overlay.set_focusable(false);
        }
        if let Some(target) = previous {
            crate::paste::force_foreground(target);
        }
        if let CaptureEvent::Record(region) = event {
            self.start_screen_recording(*region, cx);
        }
        if let CaptureEvent::Chosen { frame, region, .. } = event {
            // De dónde sale volando hacia el estante, en la ventana.
            let session = &view.read(cx).session;
            let (monitor, scale, offset) = (
                session.frozen.frame.bounds,
                session.frozen.scale,
                session.frozen.offset,
            );
            let from = Rect::new(
                (region.x - monitor.x) as f32 / scale + offset.0,
                (region.y - monitor.y) as f32 / scale + offset.1,
                region.width as f32 / scale,
                region.height as f32 / scale,
            );
            let (frame, region) = (frame.clone(), *region);
            cx.spawn(async move |this, cx| {
                let started = Instant::now();
                let saved = cx.background_spawn(async move { save(&frame, region) }).await;
                let _ = this.update(cx, |pill, cx| pill.capture_saved(saved, started, from, cx));
            })
            .detach();
        }
        cx.notify();
    }

    pub(crate) fn capture_saved(
        &mut self,
        saved: Result<Saved, String>,
        started: Instant,
        from: Rect,
        cx: &mut Context<Self>,
    ) {
        match saved {
            Ok(saved) => {
                println!(
                    "captura: {}×{} copiada y guardada en {} ({} ms)",
                    saved.width,
                    saved.height,
                    saved.path.display(),
                    started.elapsed().as_millis()
                );
                self.show_shelf(saved, from);
            }
            Err(error) => eprintln!("captura: {error}"),
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Un monitor de 200×100 físicos en (100, 50), al 200 %.
    fn session(windows: Vec<Candidate>) -> Session {
        let frame = Frame::new(PhysRect::new(100, 50, 200, 100), vec![0; 200 * 100 * 4]);
        let buffer = image::RgbaImage::from_raw(200, 100, frame.bgra.clone()).unwrap();
        Session::new(
            Frozen {
                frame,
                image: Arc::new(RenderImage::new([image::Frame::new(buffer)])),
                scale: 2.0,
                windows,
                took: std::time::Duration::ZERO,
                offset: (0.0, 0.0),
            },
            (0.0, 0.0),
            None,
        )
    }

    #[test]
    fn de_logico_a_fisico_con_escala() {
        let s = session(Vec::new());
        assert_eq!(s.screen(), Rect::new(0.0, 0.0, 100.0, 50.0));
        let phys = s.physical(&Rect::new(10.0, 10.0, 20.0, 5.0));
        assert_eq!((phys.x, phys.y, phys.width, phys.height), (120, 70, 40, 10));
        assert_eq!(s.pixel_at((10.4, 3.0)), (20, 6));
    }

    #[test]
    fn otra_escala_conserva_los_pixeles_fisicos() {
        // Estirada sobre una pantalla al 125 %, la ventana pasa de 2 a 1,25.
        let mut s = session(vec![Candidate { rect: Rect::new(10.0, 10.0, 20.0, 5.0), title: "Notas".into() }]);
        let before = s.physical(&s.frozen.windows[0].rect);
        s.frozen.rescale(1.25);
        let after = s.physical(&s.frozen.windows[0].rect);
        assert_eq!((before.x, before.y, before.width, before.height), (after.x, after.y, after.width, after.height));
        assert_eq!(s.screen(), Rect::new(0.0, 0.0, 160.0, 80.0));
    }

    #[test]
    fn region_ventana_o_pantalla() {
        let mut s = session(vec![Candidate {
            rect: Rect::new(5.0, 5.0, 30.0, 20.0),
            title: "Notas".into(),
        }]);
        s.cursor = (10.0, 10.0);
        assert_eq!(s.pick(), Pick::Window(Rect::new(5.0, 5.0, 30.0, 20.0), "Notas".into()));
        s.cursor = (90.0, 40.0);
        assert_eq!(s.pick(), Pick::Screen);
        // Un temblor de 2 px al hacer clic no es una región.
        s.press = Some((89.0, 39.0));
        assert_eq!(s.pick(), Pick::Screen);
        s.press = Some((60.0, 30.0));
        assert_eq!(s.pick(), Pick::Region(Rect::new(60.0, 30.0, 30.0, 10.0)));
    }
}
