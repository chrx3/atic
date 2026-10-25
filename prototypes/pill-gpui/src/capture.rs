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
use crate::live_loupe::LiveLoupe;
use crate::screen_recording::RecordOptions;
use crate::text_select::{self, Mapping, TextHost, TextLayer};

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
    /// Lo congelado en las coordenadas de pantalla del sistema (las de los
    /// monitores y las ventanas): píxeles en Windows, puntos en macOS.
    pub area: PhysRect,
}

impl Drop for Frozen {
    fn drop(&mut self) {
        crate::release::image(self.image.clone());
    }
}

impl Frozen {
    /// Píxeles del cuadro por unidad de pantalla: 1 en Windows (los dos son
    /// físicos), la escala de Retina en macOS (pantalla en puntos, cuadro en
    /// píxeles).
    pub fn density(&self) -> f32 {
        if self.area.width == 0 {
            return 1.0;
        }
        self.frame.bounds.width as f32 / self.area.width as f32
    }

    /// Un rectángulo de pantalla (monitor, ventana) en lógico del área.
    fn screen_to_logical(&self, rect: &PhysRect) -> Rect {
        screen_to_logical(rect, self.area, self.density(), self.scale)
    }

    /// Un rectángulo del cuadro en coordenadas de pantalla: lo que piden
    /// `capture_rect` y la grabación.
    pub fn to_screen(&self, region: PhysRect) -> PhysRect {
        let k = self.density();
        if (k - 1.0).abs() < f32::EPSILON {
            return region;
        }
        let (bx, by) = (self.frame.bounds.x as f32, self.frame.bounds.y as f32);
        let left = self.area.x + ((region.x as f32 - bx) / k).round() as i32;
        let top = self.area.y + ((region.y as f32 - by) / k).round() as i32;
        let right = self.area.x + ((region.right() as f32 - bx) / k).round() as i32;
        let bottom = self.area.y + ((region.bottom() as f32 - by) / k).round() as i32;
        PhysRect::from_ltrb(left, top, right, bottom)
    }
}

/// De pantalla a lógico del área congelada, con `k` píxeles del cuadro por
/// unidad de pantalla y `scale` píxeles por lógico.
fn screen_to_logical(rect: &PhysRect, area: PhysRect, k: f32, scale: f32) -> Rect {
    let unit = k / scale;
    Rect::new(
        (rect.x - area.x) as f32 * unit,
        (rect.y - area.y) as f32 * unit,
        rect.width as f32 * unit,
        rect.height as f32 * unit,
    )
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

    /// Lo mismo desde una ventana espejo corrida `shift` (`spread.rs`).
    pub fn local_in(&self, p: gpui::Point<gpui::Pixels>, shift: (f32, f32)) -> (f32, f32) {
        let (x, y) = self.local(p);
        (x + shift.0, y + shift.1)
    }

    /// Un punto de pantalla (coordenadas del sistema), en las de la foto.
    pub fn from_screen(&self, (x, y): (f64, f64)) -> (f32, f32) {
        let unit = self.density() / self.scale;
        (
            (x as f32 - self.area.x as f32) * unit,
            (y as f32 - self.area.y as f32) * unit,
        )
    }
}

/// Congela el monitor principal. Corre en un hilo de fondo.
/// Congela `area` (píxeles físicos de pantalla): un monitor o, con todos a la
/// misma escala, el escritorio entero.
pub fn freeze(scale: f32, area: PhysRect) -> Result<Frozen, String> {
    // Sin el permiso, macOS entrega el fondo de pantalla sin las otras apps:
    // se pide (o se abre su panel) y no se congela nada.
    #[cfg(target_os = "macos")]
    if !crate::platform::permissions::ensure_screen_capture() {
        return Err(crate::i18n::t("pill.capture.noPermission").into());
    }
    let started = Instant::now();
    let all = monitors::enumerate();
    let frame = atic_capture::engine::capture_rect(area, include_cursor()).map_err(|error| error.to_string())?;
    let k = if area.width == 0 { 1.0 } else { frame.bounds.width as f32 / area.width as f32 };
    let to_logical = |rect: &PhysRect| screen_to_logical(rect, area, k, scale);
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
    let image = image_of(&frame).ok_or(crate::i18n::t("pill.capture.badFrame"))?;
    Ok(Frozen {
        frame,
        image,
        scale,
        windows,
        took: started.elapsed(),
        offset: (0.0, 0.0),
        area,
    })
}

/// Lo que tarda la pill en recogerse antes de la foto.
pub(crate) const PHOTO_SETTLE: std::time::Duration = std::time::Duration::from_millis(380);

/// Lo que tarda macOS en sacar de las capturas una ventana recién excluida
/// (`setSharingType:`): sin esperar, a veces la foto salía con la pill.
pub(crate) const EXCLUDE_SETTLE: std::time::Duration = std::time::Duration::from_millis(120);

/// Suelta la textura de una foto que ya no se muestra.
fn release_photo(image: Arc<RenderImage>) {
    crate::release::image(image);
}

/// GPUI dibuja BGRA tal cual: el buffer del frame sirve sin convertir.
pub(crate) fn image_of(frame: &Frame) -> Option<Arc<RenderImage>> {
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

impl Drop for Session {
    fn drop(&mut self) {
        if let Some((_, image)) = self.with_pill.take() {
            crate::release::image(image);
        }
    }
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
    pub record_options: RecordOptions,
    /// Lo elegido para grabar (lógico y físico) y desde cuándo corre la cuenta.
    countdown: Option<(Rect, PhysRect, Instant)>,
    /// La foto con la pill mientras se ve la que no la tiene, para volver a ella.
    with_pill: Option<(Frame, Arc<RenderImage>)>,
    /// En vivo, los píxeles bajo el cursor para la lupa; `at` es el píxel
    /// del cuadro en que se leyeron.
    loupe: LiveLoupe,
    /// Cada monitor, en coordenadas de la foto: la ayuda va en el del cursor.
    screens: Vec<Rect>,
}

/// El modo con que abre la mira: el último que se usó.
fn mode_file() -> Option<PathBuf> {
    crate::paths::file("capture-mode.txt")
}

/// `capture_include_cursor`: el puntero sale en la foto.
pub(crate) fn include_cursor() -> bool {
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
    /// La mira pasa a grabar, con las opciones de la última vez.
    pub fn enter_record(&mut self) {
        self.record = true;
        self.record_options = RecordOptions::load();
    }

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
            record_options: RecordOptions::default(),
            countdown: None,
            with_pill: None,
            loupe: LiveLoupe::default(),
            screens: Vec::new(),
        }
        .with_screens()
    }

    fn with_screens(mut self) -> Self {
        self.screens = monitors::enumerate().iter().map(|monitor| self.frozen.screen_to_logical(&monitor.bounds)).collect();
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
    /// capturas, así que sale sin la pill ni la mira. Devuelve la foto de
    /// antes: quien no la guarde la suelta (`release_photo`), o su textura
    /// queda en la GPU.
    fn refreeze(&mut self) -> Result<(Frame, Arc<RenderImage>), String> {
        let started = Instant::now();
        let frame = atic_capture::engine::capture_rect(self.frozen.area, include_cursor())
            .map_err(|error| error.to_string())?;
        let image = image_of(&frame).ok_or(crate::i18n::t("pill.capture.badFrame"))?;
        let old_image = std::mem::replace(&mut self.frozen.image, image);
        let old_frame = std::mem::replace(&mut self.frozen.frame, frame);
        self.frozen.took = started.elapsed();
        Ok((old_frame, old_image))
    }

    /// La foto de ahora es otra: la de la pill ya no vale.
    fn forget_pill(&mut self) {
        self.pill_shown = false;
        if let Some((_, image)) = self.with_pill.take() {
            crate::release::image(image);
        }
    }

    /// En vivo: relee al tiro los píxeles alrededor del cursor para la lupa.
    pub fn refresh_loupe(&mut self) {
        if !self.live {
            return;
        }
        let (pixel, rect) = self.loupe_rect();
        let k = self.frozen.density();
        self.loupe.set(pixel, read_loupe(rect, k));
    }

    /// El píxel del cuadro bajo el cursor y lo que hay que leer para la lupa.
    fn loupe_rect(&self) -> ((i32, i32), PhysRect) {
        let pixel = self.pixel_at(self.cursor);
        let half = LOUPE_PIXELS / 2;
        let bounds = self.frozen.frame.bounds;
        // En pantalla: en macOS son puntos, así que los 13 píxeles caben en
        // menos (y se pide uno más por el redondeo).
        let k = self.frozen.density();
        let cells = |n: i32| ((n as f32 / k).ceil() as i32 + if k > 1.0 { 1 } else { 0 }) as u32;
        let rect = self.frozen.to_screen(PhysRect::new(bounds.x + pixel.0 - half, bounds.y + pixel.1 - half, 1, 1));
        (pixel, PhysRect::new(rect.x, rect.y, cells(LOUPE_PIXELS), cells(LOUPE_PIXELS)))
    }

    /// Lo que `refresh_loupe`, pero en segundo plano: lo usa el mouse.
    fn request_loupe(&mut self, cx: &mut Context<CaptureView>) {
        if !self.live {
            return;
        }
        let (pixel, rect) = self.loupe_rect();
        let k = self.frozen.density();
        self.loupe.request(
            pixel,
            move || read_loupe(rect, k),
            |view: &mut CaptureView| &mut view.session.loupe,
            cx,
        );
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
                return Pick::Region(snap_to_screens(Rect::new(x0, y0, x1 - x0, y1 - y0), &self.screens));
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
            // Por coordenadas del cuadro: la lupa trae su propio origen.
            let loupe = self.loupe.frame.as_ref()?;
            let bounds = self.frozen.frame.bounds;
            return loupe.pixel_rgba(bounds.x + pixel.0 - loupe.bounds.x, bounds.y + pixel.1 - loupe.bounds.y);
        }
        self.frozen.frame.pixel_rgba(pixel.0, pixel.1)
    }
}

/// Cuánto puede faltarle al cursor para contar como pegado al borde.
const EDGE_SNAP: f32 = 3.0;

/// Una selección que cruza pantallas desalineadas o de otro tamaño: el cursor
/// no llega al borde de arriba de una estando en la otra. Cada lado pegado al
/// borde de una pantalla que toca se estira hasta el más lejano de todas las
/// que toca: de esquina a esquina sale el escritorio entero.
fn snap_to_screens(rect: Rect, screens: &[Rect]) -> Rect {
    let touched: Vec<Rect> = screens
        .iter()
        .copied()
        .filter(|s| rect.x < s.right() && rect.right() > s.x && rect.y < s.bottom() && rect.bottom() > s.y)
        .collect();
    if touched.len() < 2 {
        return rect;
    }
    let near = |a: f32, b: f32| (a - b).abs() <= EDGE_SNAP;
    let pinned = |edge: fn(&Rect) -> f32, value: f32| touched.iter().any(|s| near(edge(s), value));
    let fold = |edge: fn(&Rect) -> f32, pick: fn(f32, f32) -> f32| touched.iter().map(edge).fold(edge(&touched[0]), pick);
    let (mut left, mut top, mut right, mut bottom) = (rect.x, rect.y, rect.right(), rect.bottom());
    if pinned(|s| s.x, left) {
        left = fold(|s| s.x, f32::min);
    }
    if pinned(|s| s.y, top) {
        top = fold(|s| s.y, f32::min);
    }
    if pinned(|s| s.right(), right) {
        right = fold(|s| s.right(), f32::max);
    }
    if pinned(|s| s.bottom(), bottom) {
        bottom = fold(|s| s.bottom(), f32::max);
    }
    Rect::new(left, top, right - left, bottom - top)
}

/// Los píxeles para la lupa en vivo, `rect` en coordenadas de pantalla.
///
/// Sin `CAPTUREBLT`: con él, Windows sincroniza con el compositor en cada
/// lectura. El overlay no sale igual (está excluido de capturas).
fn read_loupe(rect: PhysRect, k: f32) -> Option<Frame> {
    let frame = atic_capture::engine::capture_rect_without_layered(rect).ok()?;
    Some(to_density(frame, rect, k))
}

/// Una lectura de una pantalla menos densa que la foto (la externa junto a
/// la Retina) sale con menos píxeles: se repiten para que calce con la grilla
/// de la foto, que va a la escala mayor.
fn to_density(frame: Frame, rect: PhysRect, k: f32) -> Frame {
    let w = (rect.width as f32 * k).round().max(1.0) as u32;
    let h = (rect.height as f32 * k).round().max(1.0) as u32;
    let (sw, sh) = (frame.width(), frame.height());
    if (sw, sh) == (w, h) || sw == 0 || sh == 0 {
        return frame;
    }
    let mut bgra = vec![0u8; w as usize * h as usize * 4];
    for y in 0..h {
        let sy = (y * sh / h) as usize;
        for x in 0..w {
            let sx = (x * sw / w) as usize;
            let from = (sy * sw as usize + sx) * 4;
            let to = (y as usize * w as usize + x as usize) * 4;
            bgra[to..to + 4].copy_from_slice(&frame.bgra[from..from + 4]);
        }
    }
    let bounds = PhysRect::new(
        (rect.x as f32 * k).round() as i32,
        (rect.y as f32 * k).round() as i32,
        w,
        h,
    );
    Frame::new(bounds, bgra)
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
    let cropped = frame.crop(region).ok_or(crate::i18n::t("pill.capture.outside"))?;
    let png = cropped.to_png().map_err(|error| error.to_string())?;
    let dir = captures_dir().ok_or(crate::i18n::t("pill.capture.noDir"))?;
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

actions!(
    capture,
    [Cancel, WholeScreen, ToggleLive, ToggleDraw, TogglePill, ToggleMic, ToggleSystem, ToggleText, SelectAllText, CopyText]
);

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
        KeyBinding::new("m", ToggleMic, context),
        KeyBinding::new("a", ToggleSystem, context),
        // G (como en la Pizarra) o T: elegir texto sobre la foto (`text_select.rs`).
        KeyBinding::new("g", ToggleText, context),
        KeyBinding::new("t", ToggleText, context),
        KeyBinding::new(&crate::keymap::primary("ctrl-a"), SelectAllText, context),
        KeyBinding::new(&crate::keymap::primary("ctrl-c"), CopyText, context),
    ]);
}

/// Lado de la lupa en píxeles físicos: impar, para que haya un centro.
const LOUPE_PIXELS: i32 = 13;
const LOUPE_CELL: f32 = 9.0;
const LOUPE_OFFSET: f32 = 22.0;
const DIM: f32 = 0.42;
/// La cuenta antes de grabar.
const COUNTDOWN: std::time::Duration = std::time::Duration::from_secs(3);
/// El círculo con el número de la cuenta.
const COUNTDOWN_SIZE: f32 = 112.0;
/// El borde de lo elegido para grabar: el rojo de grabar de Reuniones.
const RECORD_EDGE: u32 = crate::meetings::RECORD_RED;

pub enum CaptureEvent {
    /// Lo elegido, en físico, con el frame para recortarlo fuera de la UI.
    /// `draw`: abrir la pizarra sobre la región antes de copiar. `rect`: lo
    /// mismo en lógico de la foto, para la animación al estante.
    Chosen {
        frame: Frame,
        region: PhysRect,
        rect: Rect,
        draw: bool,
    },
    /// Grabar lo elegido, en físico.
    Record { region: PhysRect, options: RecordOptions },
    Cancelled,
}

pub struct CaptureView {
    pub session: Session,
    focus: FocusHandle,
    meter: crate::meter::Meter,
    /// El texto de la foto congelada: se lee una vez (al entrar al modo
    /// texto) y queda mientras la foto no cambie.
    text: Option<TextLayer>,
    /// Modo texto (T): los clics eligen texto en vez de capturar.
    text_mode: bool,
}

impl EventEmitter<CaptureEvent> for CaptureView {}

impl TextHost for CaptureView {
    fn text_layer(&mut self) -> Option<&mut TextLayer> {
        self.text.as_mut()
    }
}

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
            text: None,
            text_mode: false,
        }
    }

    /// Soltar el botón: lo elegido se captura.
    fn release(&mut self, event: &MouseUpEvent, cx: &mut Context<Self>) {
        // El soltar del clic que abrió la captura (desde la gota, que
        // abre al presionar) no es una elección: capturaba al tiro.
        if self.session.press.is_none() {
            return;
        }
        let pick = self.session.pick();
        self.session.press = None;
        let draw = self.session.draw || event.modifiers.shift;
        self.choose(pick, draw, cx);
    }

    /// macOS, con ventanas espejo: el cursor se lee del sistema. Un arrastre
    /// le sigue mandando los eventos a la ventana donde empezó (y GPUI no
    /// entrega los que caen fuera de ella), y sin apretar, la ventana de otra
    /// pantalla no siempre recibe los movimientos: la lupa se quedaba en la
    /// pantalla de la pill.
    #[cfg(target_os = "macos")]
    pub fn follow_drag(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |view, cx| loop {
            cx.background_executor().timer(std::time::Duration::from_millis(8)).await;
            let alive = view.update(cx, |view, cx| {
                if view.text_mode {
                    return;
                }
                let session = &mut view.session;
                if session.countdown.is_some() {
                    return;
                }
                let cursor = session.frozen.from_screen(crate::win::cursor_global());
                if cursor != session.cursor {
                    session.cursor = cursor;
                    session.request_loupe(cx);
                    cx.notify();
                }
            });
            if alive.is_err() {
                break;
            }
        })
        .detach();
    }

    /// Dónde está la foto en la ventana, para la capa de texto.
    fn text_map(&self) -> Mapping {
        let frozen = &self.session.frozen;
        Mapping::new(frozen.scale, frozen.offset, (frozen.frame.width() as f32, frozen.frame.height() as f32))
    }

    /// Entra al modo texto: en vivo, congela antes (el texto se lee de una
    /// foto). El OCR corre una sola vez por foto. `false` si no se pudo.
    fn enter_text(&mut self, cx: &mut Context<Self>) -> bool {
        if self.session.record || self.session.countdown.is_some() {
            return false;
        }
        if self.session.live {
            match self.session.refreeze() {
                Ok((_, old)) => {
                    release_photo(old);
                    self.session.live = false;
                    self.session.forget_pill();
                    self.text = None;
                }
                Err(error) => {
                    eprintln!("captura: no se pudo congelar para leer el texto: {error}");
                    return false;
                }
            }
        }
        self.session.press = None;
        match self.text.as_mut() {
            Some(layer) => layer.replay(),
            None => self.text = Some(text_select::start(&self.session.frozen.frame, cx)),
        }
        self.text_mode = true;
        cx.notify();
        true
    }

    /// Vuelve a la mira (lo elegido queda para la próxima vez).
    fn exit_text(&mut self, cx: &mut Context<Self>) {
        self.text_mode = false;
        if let Some(layer) = self.text.as_mut() {
            layer.release();
        }
        cx.notify();
    }

    fn toggle_text(&mut self, _: &ToggleText, _: &mut Window, cx: &mut Context<Self>) {
        if self.text_mode {
            self.exit_text(cx);
        } else {
            self.enter_text(cx);
        }
    }

    /// Ctrl+A: todo el texto (desde la mira, entra al modo texto).
    fn select_all_text(&mut self, _: &SelectAllText, _: &mut Window, cx: &mut Context<Self>) {
        if !self.text_mode && !self.enter_text(cx) {
            return;
        }
        if let Some(layer) = self.text.as_mut() {
            layer.select_all();
        }
        cx.notify();
    }

    fn copy_text(&mut self, _: &CopyText, _: &mut Window, cx: &mut Context<Self>) {
        if self.text_mode {
            text_select::copy(self, false, cx);
        }
    }

    fn choose(&mut self, pick: Pick, draw: bool, cx: &mut Context<Self>) {
        let rect = self.session.pick_rect(&pick);
        let region = self.session.physical(&rect);
        if self.session.record {
            self.start_countdown(rect, region, cx);
            return;
        }
        // En vivo la foto es de ahora: el overlay está excluido de las
        // capturas, así que no hace falta esconderlo antes.
        let (frame, region) = if self.session.live {
            // La foto en vivo es justo lo elegido, a la escala de su pantalla:
            // con pantallas mezcladas puede no ser la de la sesión (la externa
            // a 1x junto a la Retina), así que se recorta entera y no con
            // `region`, que va en la grilla de la sesión.
            match atic_capture::engine::capture_rect(self.session.frozen.to_screen(region), include_cursor()) {
                Ok(frame) => {
                    let bounds = frame.bounds;
                    (frame, bounds)
                }
                Err(error) => {
                    eprintln!("captura en vivo: {error}");
                    (self.session.frozen.frame.clone(), region)
                }
            }
        } else {
            // Solo la región: copiar la pantalla entera eran ~20 MB más en
            // el momento en que más memoria se usa.
            let frozen = &self.session.frozen.frame;
            (frozen.crop(region).unwrap_or_else(|| frozen.clone()), region)
        };
        cx.emit(CaptureEvent::Chosen {
            frame,
            region,
            rect,
            draw,
        });
    }

    /// F: congelar lo que se ve ahora, o volver a la pantalla viva.
    fn toggle_live(&mut self, _: &ToggleLive, _: &mut Window, cx: &mut Context<Self>) {
        if self.text_mode {
            return;
        }
        // La foto cambia (o deja de verse): su texto ya no vale.
        self.text = None;
        let session = &mut self.session;
        if session.live {
            match session.refreeze() {
                Ok((_, old)) => {
                    session.live = false;
                    // La foto nueva es de ahora y sin la pill: la de antes ya no vale.
                    match &session.with_pill {
                        Some((_, kept)) if Arc::ptr_eq(kept, &old) => {}
                        _ => release_photo(old),
                    }
                    session.forget_pill();
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
        if self.session.record {
            self.toggle_option(|options| options.show_pill = !options.show_pill, cx);
            return;
        }
        if self.text_mode {
            return;
        }
        let session = &mut self.session;
        if !session.can_toggle_pill() {
            return;
        }
        if session.pill_shown {
            match session.refreeze() {
                Ok(with) => {
                    session.with_pill = Some(with);
                    session.pill_shown = false;
                }
                Err(error) => eprintln!("captura: no se pudo congelar sin la pill: {error}"),
            }
        } else if let Some((frame, image)) = session.with_pill.take() {
            session.frozen.frame = frame;
            // La foto sin la pill no se vuelve a usar: P la toma de nuevo.
            release_photo(std::mem::replace(&mut session.frozen.image, image));
            session.pill_shown = true;
            // Desde en vivo queda congelada en esa foto, sin cambiar el modo
            // con que abre la próxima vez.
            session.live = false;
        }
        // Otra foto: su texto se lee de nuevo.
        self.text = None;
        cx.notify();
    }

    pub fn set_record(&mut self, cx: &mut Context<Self>) {
        self.session.enter_record();
        cx.notify();
    }

    /// Lo elegido queda marcado sobre la pantalla viva mientras cuenta.
    fn start_countdown(&mut self, rect: Rect, region: PhysRect, cx: &mut Context<Self>) {
        if self.session.countdown.is_some() {
            return;
        }
        self.session.countdown = Some((rect, region, Instant::now()));
        cx.spawn(async move |view, cx| loop {
            cx.background_executor().timer(std::time::Duration::from_millis(100)).await;
            let done = view
                .update(cx, |view, cx| {
                    let Some((_, _, since)) = view.session.countdown else {
                        return true;
                    };
                    if since.elapsed() >= COUNTDOWN {
                        view.record_now(cx);
                        return true;
                    }
                    cx.notify();
                    false
                })
                .unwrap_or(true);
            if done {
                break;
            }
        })
        .detach();
        cx.notify();
    }

    fn record_now(&mut self, cx: &mut Context<Self>) {
        if let Some((_, region, _)) = self.session.countdown.take() {
            cx.emit(CaptureEvent::Record {
                // La grabación va en coordenadas de pantalla (puntos en macOS).
                region: self.session.frozen.to_screen(region),
                options: self.session.record_options,
            });
        }
    }

    /// M, A y P en modo grabar: qué entra al video. Se recuerda.
    fn toggle_option(&mut self, flip: impl FnOnce(&mut RecordOptions), cx: &mut Context<Self>) {
        if !self.session.record || self.session.countdown.is_some() {
            return;
        }
        flip(&mut self.session.record_options);
        self.session.record_options.save();
        cx.notify();
    }

    /// Esc: en modo texto borra la selección y, sin ella, vuelve a la mira.
    fn cancel(&mut self, _: &Cancel, _: &mut Window, cx: &mut Context<Self>) {
        if self.text_mode {
            if self.text.as_mut().is_some_and(TextLayer::escape) {
                cx.notify();
            } else {
                self.exit_text(cx);
            }
            return;
        }
        cx.emit(CaptureEvent::Cancelled);
    }

    fn whole_screen(&mut self, _: &WholeScreen, _: &mut Window, cx: &mut Context<Self>) {
        // En modo texto, Enter copia lo elegido.
        if self.text_mode {
            text_select::copy(self, false, cx);
            return;
        }
        if self.session.countdown.is_some() {
            self.record_now(cx);
            return;
        }
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
        if self.loupe.is_empty() {
            return;
        }
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
        .font_family(crate::fonts::UI)
        .child(text.into())
}

impl Render for CaptureView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.render_shifted((0.0, 0.0), window, cx)
    }
}

impl crate::spread::Spread for CaptureView {
    /// Las otras pantallas dibujan lo mismo con la foto corrida. El modo
    /// texto queda solo en la del overlay.
    fn render_at(&mut self, shift: (f32, f32), window: &mut Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        if shift != (0.0, 0.0) && self.text_mode {
            return div().into_any_element();
        }
        let home = self.session.frozen.offset;
        self.session.frozen.offset = (home.0 - shift.0, home.1 - shift.1);
        let element = self.render_shifted(shift, window, cx);
        self.session.frozen.offset = home;
        element
    }
}

impl CaptureView {
    fn render_shifted(&mut self, shift: (f32, f32), window: &mut Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        self.meter.frame(Instant::now());
        if let Some((rect, _, since)) = self.session.countdown {
            return self.countdown_view(rect, since, cx);
        }
        if self.text_mode {
            return self.text_view(window, cx);
        }
        let session = &self.session;
        let screen = session.screen();
        let help = session.cursor_screen();
        let pick = session.pick();
        let selection = match &pick {
            Pick::Screen => None,
            other => Some(session.pick_rect(other)),
        };
        let cursor = session.cursor;
        // En vivo, la lupa se centra donde se leyó lo último que llegó: el
        // cuadro sigue al cursor al tiro y los píxeles lo alcanzan después.
        let pixel = if session.live { session.loupe.at } else { session.pixel_at(cursor) };
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
            .on_action(cx.listener(|view, _: &ToggleMic, _, cx| {
                view.toggle_option(|options| options.mic = !options.mic, cx)
            }))
            .on_action(cx.listener(|view, _: &ToggleSystem, _, cx| {
                view.toggle_option(|options| options.system = !options.system, cx)
            }))
            .on_action(cx.listener(|view, _: &ToggleDraw, _, cx| {
                view.session.draw = !view.session.draw;
                cx.notify();
            }))
            .on_action(cx.listener(Self::toggle_text))
            .on_action(cx.listener(Self::select_all_text))
            .size_full()
            .relative()
            .cursor(CursorStyle::Crosshair)
            .on_mouse_move(cx.listener(move |view, event: &MouseMoveEvent, _, cx| {
                view.session.cursor = view.session.frozen.local_in(event.position, shift);
                view.session.request_loupe(cx);
                cx.notify();
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |view, event: &MouseDownEvent, _, cx| {
                    let p = view.session.frozen.local_in(event.position, shift);
                    view.session.cursor = p;
                    view.session.press = Some(p);
                    cx.stop_propagation();
                    cx.notify();
                }),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(move |view, event: &MouseUpEvent, _, cx| {
                    view.session.cursor = view.session.frozen.local_in(event.position, shift);
                    view.release(event, cx);
                }),
            )
            // Soltar en otra pantalla: el evento le llega a la ventana donde
            // empezó el arrastre, fuera de ella. El cursor ya lo siguió
            // `follow_drag`.
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|view, event: &MouseUpEvent, _, cx| view.release(event, cx)),
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
                        let yes = |on: bool| if on { crate::i18n::t("pill.capture.yes") } else { crate::i18n::t("pill.capture.no") };
                        let options = session.record_options;
                        crate::i18n::tf(
                            "pill.capture.recordBar",
                            &[("mic", &yes(options.mic)), ("system", &yes(options.system)), ("pill", &yes(options.show_pill))],
                        )
                    } else {
                        let mode = if session.live {
                            crate::i18n::t("pill.capture.live").to_string()
                        } else {
                            crate::i18n::tf("pill.capture.frozen", &[("ms", &session.frozen.took.as_millis())])
                        };
                        let draw = if session.draw {
                            crate::i18n::t("pill.capture.drawOn")
                        } else {
                            crate::i18n::t("pill.capture.drawOff")
                        };
                        let pill = if !session.can_toggle_pill() {
                            ""
                        } else if session.pill_shown {
                            crate::i18n::t("pill.capture.hidePill")
                        } else {
                            crate::i18n::t("pill.capture.showPill")
                        };
                        crate::i18n::tf("pill.capture.bar", &[("mode", &mode), ("pill", &pill), ("draw", &draw)])
                    })),
            )
            .into_any_element()
    }
}

impl CaptureView {
    /// El modo texto: la foto un poco más oscura, las palabras encendidas y
    /// los clics eligiendo texto como en un documento (`text_select.rs`).
    fn text_view(&mut self, window: &mut Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        let now = Instant::now();
        let map = self.text_map();
        let session = &self.session;
        let screen = session.screen();
        let offset = session.frozen.offset;
        let help = session.cursor_screen();
        let (paint, toolbar, cursor, status) = match self.text.as_ref() {
            Some(layer) => {
                // Solo mientras algo se anima: quieta no pide cuadros.
                if layer.animating(now) {
                    window.request_animation_frame();
                }
                (Some(layer.paint(map, now)), layer.toolbar(map, cx), layer.cursor_style(), layer.status())
            }
            None => (None, None, CursorStyle::Arrow, None),
        };
        let hint = status.unwrap_or_else(|| {
            crate::keymap::hint_owned(
                crate::i18n::t("pill.capture.textHint").to_string(),
            )
        });
        div()
            .id("capture")
            .track_focus(&self.focus)
            .key_context(KEY_CONTEXT)
            .on_action(cx.listener(Self::cancel))
            .on_action(cx.listener(Self::whole_screen))
            .on_action(cx.listener(Self::toggle_text))
            .on_action(cx.listener(Self::select_all_text))
            .on_action(cx.listener(Self::copy_text))
            .size_full()
            .relative()
            .cursor(cursor)
            .on_mouse_move(cx.listener(|view, event: &MouseMoveEvent, _, cx| {
                view.session.cursor = view.session.frozen.local(event.position);
                let map = view.text_map();
                if let Some(layer) = view.text.as_mut() {
                    if layer.moved(map, event.position) {
                        cx.notify();
                    }
                }
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|view, event: &MouseDownEvent, _, cx| {
                    let map = view.text_map();
                    if let Some(layer) = view.text.as_mut() {
                        layer.press(map, event.position, event.click_count, event.modifiers.shift);
                    }
                    cx.stop_propagation();
                    cx.notify();
                }),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|view, _: &MouseUpEvent, _, cx| {
                    if let Some(layer) = view.text.as_mut() {
                        layer.release();
                    }
                    cx.notify();
                }),
            )
            // Clic derecho: de vuelta a la mira.
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|view, _: &MouseDownEvent, _, cx| view.exit_text(cx)),
            )
            .child(
                img(session.frozen.image.clone())
                    .absolute()
                    .left(px(offset.0))
                    .top(px(offset.1))
                    .w(px(screen.w))
                    .h(px(screen.h)),
            )
            .children(paint.map(|paint| {
                canvas(|_, _, _| {}, move |_, _, window, _| paint.paint(window))
                    .absolute()
                    .size_full()
            }))
            .children(toolbar)
            .child(
                div()
                    .absolute()
                    .left(px(help.x + offset.0))
                    .top(px(help.y + 10.0 + offset.1))
                    .w(px(help.w))
                    .flex()
                    .justify_center()
                    .child(chip(hint)),
            )
            .into_any_element()
    }

    /// La cuenta antes de grabar: la pantalla viva, oscura salvo lo elegido,
    /// con el número encima. Espacio empieza ya; Esc o clic derecho, nada.
    fn countdown_view(&self, rect: Rect, since: Instant, cx: &mut Context<Self>) -> gpui::AnyElement {
        let session = &self.session;
        let (screen, offset) = (session.screen(), session.frozen.offset);
        let left = COUNTDOWN.saturating_sub(since.elapsed());
        let number = (left.as_secs_f32().ceil() as u32).max(1);
        let help = session.cursor_screen();
        let paint = Paint {
            offset,
            screen,
            selection: Some(rect),
            window_hint: false,
            record: true,
            cursor: (0.0, 0.0),
            crosshair: false,
            loupe: Vec::new(),
            loupe_origin: (0.0, 0.0),
        };
        div()
            .id("capture")
            .track_focus(&self.focus)
            .key_context(KEY_CONTEXT)
            .on_action(cx.listener(Self::cancel))
            .on_action(cx.listener(Self::whole_screen))
            .size_full()
            .relative()
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|_, _: &MouseDownEvent, _, cx| cx.emit(CaptureEvent::Cancelled)),
            )
            .child(
                canvas(|_, _, _| {}, move |_, _, window, _| paint.paint(window))
                    .absolute()
                    .size_full(),
            )
            .child(
                div()
                    .absolute()
                    .left(px(rect.x + offset.0))
                    .top(px(rect.y + offset.1))
                    .w(px(rect.w))
                    .h(px(rect.h))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .size(px(COUNTDOWN_SIZE))
                            .rounded_full()
                            .bg(gpui::black().opacity(0.6))
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_color(gpui::white())
                            .text_size(px(COUNTDOWN_SIZE * 0.55))
                            .font_family(crate::fonts::UI)
                            .child(number.to_string()),
                    ),
            )
            .child(
                div()
                    .absolute()
                    .left(px(help.x + offset.0))
                    .top(px(help.y + 10.0 + offset.1))
                    .w(px(help.w))
                    .flex()
                    .justify_center()
                    .child(chip(crate::i18n::t("pill.capture.recordSoon"))),
            )
            .into_any_element()
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

/// El monitor que contiene el punto (en coordenadas de pantalla del sistema).
pub(crate) fn monitors_at(x: i32, y: i32) -> Option<PhysRect> {
    monitors::enumerate()
        .into_iter()
        .map(|monitor| monitor.bounds)
        .find(|b| x >= b.x && x < b.right() && y >= b.y && y < b.bottom())
}

pub(crate) fn to_win_rect(rect: PhysRect) -> crate::win::RECT {
    crate::win::RECT {
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
    Restoring(crate::win::RECT),
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
    ///
    /// En macOS una ventana no se ve en dos pantallas a la vez (cada una
    /// tiene sus espacios): la mira va a la pantalla del cursor, y si no es
    /// la de la pill, la ventana se lleva hasta ahí mientras dura.
    fn capture_area(&self) -> Option<(PhysRect, bool)> {
        // Con más de una pantalla, la foto es del escritorio entero y las
        // otras pantallas reciben ventanas espejo (`spread.rs`): la ventana
        // no se mueve. Copiar texto sigue en una sola.
        #[cfg(target_os = "macos")]
        if !self.capture_text {
            if let Some(area) = whole_desktop() {
                return Some((area, false));
            }
        }
        #[cfg(target_os = "macos")]
        {
            let home = self.pill_area();
            let under = self.overlay.as_ref().and_then(|overlay| overlay.screen_under_cursor()).map(|screen| {
                let m = screen.monitor;
                PhysRect::new(m.left, m.top, (m.right - m.left) as u32, (m.bottom - m.top) as u32)
            });
            return match (under, home) {
                (Some(under), Some(home)) if under != home => Some((under, true)),
                (_, Some(home)) => Some((home, false)),
                (Some(under), None) => Some((under, true)),
                (None, None) => None,
            };
        }
        #[allow(unreachable_code)]
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

    /// La foto congelada sale con la pill, y P la quita. `with_pill`: tal
    /// como está (el atajo, para mostrar lo que tiene). Si no (desde la rueda,
    /// la tira o el lanzador), primero se recoge y la foto espera a que
    /// termine de cerrarse. «Copiar texto» congela sin ella.
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
        let text = self.capture_text;
        // Lo que espera la foto: que macOS saque a la pill de las capturas o
        // que termine de recogerse.
        let settle = if text {
            self.hide_for_capture(cx);
            Some(EXCLUDE_SETTLE)
        } else {
            (!with_pill && self.collapse_for_photo(cx)).then_some(PHOTO_SETTLE)
        };
        let with_pill = !text;
        let previous = crate::paste::foreground_target();
        let scale = self.scale_factor;
        let Some((area, whole)) = self.capture_area() else {
            self.capture_pending = false;
            return;
        };
        cx.spawn_in(window, async move |this, cx| {
            if let Some(settle) = settle {
                cx.background_executor().timer(settle).await;
            }
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

    /// «Copiar texto de la pantalla»: la mira se abre ya leyendo el texto.
    pub(crate) fn start_text_select(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.capture.is_some() || self.capture_pending || self.screen_rec.active() {
            return;
        }
        self.capture_text = true;
        self.start_capture(false, window, cx);
        if !self.capture_pending && self.capture.is_none() {
            self.capture_text = false;
        }
    }

    /// Recoge la pill para la foto, sin sacarla de ella. `true` si había algo
    /// abierto: la foto espera a que termine de cerrarse (`PHOTO_SETTLE`).
    pub(crate) fn collapse_for_photo(&mut self, cx: &mut Context<Self>) -> bool {
        let now = Instant::now();
        let open = self.wheel_visible()
            || self.panel_visible()
            || self.peek.value(now) > 0.0
            || self.strip.value(now) > 0.0
            || self.usage_peek.value(now) > 0.0
            || self.tool_peek_amount(now) > 0.0;
        self.close_wheel();
        self.close_panel(false, cx);
        self.peek.set(0.0, now);
        self.strip.set(0.0, now);
        // Los vistazos de las herramientas y del uso de agentes.
        self.hand_off_peeks(crate::peeks::PeekKind::Clipboard, now);
        open
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
        let text = std::mem::take(&mut self.capture_text);
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
        //
        // En macOS lo lógico de la foto son puntos (los de todas las
        // ventanas, espejos incluidos): su escala es la de la foto, que con
        // pantallas mezcladas no es la de esta ventana.
        let scale = if cfg!(target_os = "macos") { frozen.density() } else { window.scale_factor() };
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
        if std::mem::take(&mut self.screen_rec.requested) {
            session.enter_record();
        }
        if with_pill && session.live {
            // En vivo la foto del arranque (con la pill) queda para P.
            session.with_pill = Some((session.frozen.frame.clone(), session.frozen.image.clone()));
        } else {
            session.pill_shown = with_pill;
        }
        session.refresh_loupe();
        let view = cx.new(|cx| CaptureView::new(session, cx));
        if text {
            view.update(cx, |view, cx| {
                view.enter_text(cx);
            });
        }
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
        #[cfg(target_os = "macos")]
        if let Some(origin) = self.overlay.as_ref().and_then(|overlay| overlay.origin()) {
            self.mirrors = crate::spread::Mirrors::open(&view, origin, cx);
            if self.mirrors.is_open() {
                view.update(cx, |view, cx| view.follow_drag(cx));
            }
        }
        self.capture = Some(view);
        cx.notify();
    }

    fn end_capture(&mut self, event: &CaptureEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(view) = self.capture.take() else {
            return;
        };
        self.capture_events = None;
        self.mirrors.close(cx);
        self.settle_after_tool();
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
            ..
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
        if let CaptureEvent::Record { region, options } = event {
            self.start_screen_recording(*region, *options, cx);
        }
        if let CaptureEvent::Chosen { frame, region, rect, .. } = event {
            // De dónde sale volando hacia el estante, en la ventana.
            let offset = view.read(cx).session.frozen.offset;
            let from = Rect::new(rect.x + offset.0, rect.y + offset.1, rect.w, rect.h);
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
                area: PhysRect::new(100, 50, 200, 100),
            },
            (0.0, 0.0),
            None,
        )
    }

    /// Como las de este Mac: la Retina a la izquierda y más baja.
    fn screens() -> Vec<Rect> {
        vec![Rect::new(0.0, 132.0, 1440.0, 900.0), Rect::new(1440.0, 0.0, 1920.0, 1080.0)]
    }

    #[test]
    fn de_esquina_a_esquina_sale_el_escritorio_entero() {
        // De arriba a la izquierda de la Retina a abajo a la derecha de la
        // externa (el cursor queda un pelo antes del borde).
        let rect = Rect::new(0.0, 132.0, 3359.0, 947.0);
        let snapped = snap_to_screens(rect, &screens());
        assert_eq!((snapped.x, snapped.y, snapped.right(), snapped.bottom()), (0.0, 0.0, 3360.0, 1080.0));
    }

    #[test]
    fn lo_que_no_toca_un_borde_queda_igual() {
        let rect = Rect::new(800.0, 300.0, 1200.0, 400.0);
        assert_eq!(snap_to_screens(rect, &screens()), rect);
        // En una sola pantalla no se estira nada.
        let rect = Rect::new(0.0, 132.0, 500.0, 300.0);
        assert_eq!(snap_to_screens(rect, &screens()), rect);
    }

    #[test]
    fn solo_se_estira_el_lado_pegado() {
        // Desde arriba de la Retina hasta el medio de la externa: sube hasta
        // arriba de la externa, el resto queda.
        let rect = Rect::new(600.0, 132.0, 1400.0, 500.0);
        let snapped = snap_to_screens(rect, &screens());
        assert_eq!((snapped.x, snapped.y, snapped.right(), snapped.bottom()), (600.0, 0.0, 2000.0, 632.0));
    }

    #[test]
    fn en_mac_la_pantalla_va_en_puntos_y_el_cuadro_en_pixeles() {
        // Un monitor de 100×50 puntos en (10, 0), con el cuadro a 2×.
        let frame = Frame::new(PhysRect::new(20, 0, 200, 100), vec![0; 200 * 100 * 4]);
        let buffer = image::RgbaImage::from_raw(200, 100, frame.bgra.clone()).unwrap();
        let frozen = Frozen {
            frame,
            image: Arc::new(RenderImage::new([image::Frame::new(buffer)])),
            scale: 2.0,
            windows: Vec::new(),
            took: std::time::Duration::ZERO,
            offset: (0.0, 0.0),
            area: PhysRect::new(10, 0, 100, 50),
        };
        assert_eq!(frozen.density(), 2.0);
        // Una ventana en (30, 10) puntos cae en (20, 10) lógicos.
        assert_eq!(frozen.screen_to_logical(&PhysRect::new(30, 10, 40, 20)), Rect::new(20.0, 10.0, 40.0, 20.0));
        // Y su recorte en el cuadro vuelve a esos puntos.
        let s = Session::new(frozen, (0.0, 0.0), None);
        let region = s.physical(&Rect::new(20.0, 10.0, 40.0, 20.0));
        assert_eq!((region.x, region.y, region.width, region.height), (60, 20, 80, 40));
        let back = s.frozen.to_screen(region);
        assert_eq!((back.x, back.y, back.width, back.height), (30, 10, 40, 20));
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
