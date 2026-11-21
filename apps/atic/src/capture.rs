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
    actions, canvas, div, fill, img, outline, point, prelude::*, px, rgb, size, svg, App, Bounds,
    BorderStyle, Context, CursorStyle, EventEmitter, FocusHandle, Focusable, Hsla, KeyBinding,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, RenderImage, SharedString, Window,
};

use crate::geometry::Rect;
use crate::hover::HoverExt;
use crate::live_loupe::LiveLoupe;
use crate::scroll_capture::{Step, Stitcher};
use crate::screen_recording::RecordOptions;
use crate::text_select::{self, Mapping, TextHost, TextLayer};

/// Lo que mide la mira para distinguir un clic de un arrastre.
const DRAG_MIN: f32 = 4.0;

pub struct Candidate {
    /// Lógico, en coordenadas del overlay.
    pub rect: Rect,
    pub title: SharedString,
    /// `HWND` en Windows: con él se fotografía la ventana aunque otra la tape.
    #[cfg_attr(not(windows), allow(dead_code))]
    pub hwnd: isize,
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
    #[cfg(not(target_os = "macos"))]
    pub fn local_in(&self, p: gpui::Point<gpui::Pixels>, shift: (f32, f32)) -> (f32, f32) {
        let (x, y) = self.local(p);
        (x + shift.0, y + shift.1)
    }

    /// El cursor de un evento del mouse, en coordenadas de la foto. En macOS
    /// se lee del sistema, como `follow_drag`: el evento de una ventana espejo
    /// de otra pantalla daba otro punto y la mira saltaba entre los dos.
    pub fn cursor_in(&self, p: gpui::Point<gpui::Pixels>, shift: (f32, f32)) -> (f32, f32) {
        #[cfg(target_os = "macos")]
        {
            let _ = (p, shift);
            self.from_screen(crate::win::cursor_global())
        }
        #[cfg(not(target_os = "macos"))]
        self.local_in(p, shift)
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
    // `PILL_SLOW_FREEZE=<ms>` (solo desarrollo): una foto lenta, como con el
    // equipo saturado, para probar `FREEZE_DEADLINE`.
    #[cfg(debug_assertions)]
    if let Some(ms) = std::env::var("PILL_SLOW_FREEZE").ok().and_then(|v| v.parse::<u64>().ok()) {
        std::thread::sleep(std::time::Duration::from_millis(ms));
    }
    let started = Instant::now();
    let all = monitors::enumerate();
    let frame = atic_capture::engine::capture_rect(area, include_cursor()).map_err(|error| error.to_string())?;
    let k = if area.width == 0 { 1.0 } else { frame.bounds.width as f32 / area.width as f32 };
    let to_logical = |rect: &PhysRect| screen_to_logical(rect, area, k, scale);
    // Del frente hacia atrás: la primera que contiene el cursor es la visible.
    // Solo se descarta la ventana de la pill: Ajustes, Atic Code y las demás
    // ventanas de Atic se pueden elegir como cualquier otra. En macOS su vidrio
    // y los paneles flotantes ya quedan fuera por su nivel (sobre el normal).
    let (exclude_pid, overlay) = (0, crate::win::overlay_hwnd());
    let windows = atic_capture::windows::enumerate_candidates(exclude_pid, &all)
        .into_iter()
        .filter(|candidate| overlay == 0 || candidate.hwnd != overlay)
        .filter_map(|candidate| {
            let visible = candidate.visual_bounds.intersection(&area)?;
            Some(Candidate {
                rect: to_logical(&visible),
                title: candidate.title.into(),
                hwnd: candidate.hwnd,
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
/// Lo que se espera la foto de la pantalla antes de rendirse. Con el equipo
/// libre tarda 100–250 ms; si pasa de esto, macOS o la tarjeta de video
/// están saturados y seguir esperando deja la pill sin responder.
pub(crate) const FREEZE_DEADLINE: std::time::Duration = std::time::Duration::from_secs(3);

/// Lo que espera la mira abierta una foto nueva (`Session::refreeze`).
const REFREEZE_WITHIN: std::time::Duration = std::time::Duration::from_millis(1500);

/// La foto de `area` o, si no llega antes de `FREEZE_DEADLINE`, `None` (la
/// foto que llegue después se pierde con la tarea).
pub(crate) async fn freeze_in_time(executor: gpui::BackgroundExecutor, scale: f32, area: PhysRect) -> Option<Result<Frozen, String>> {
    let photo = executor.spawn(async move { freeze(scale, area) });
    let deadline = executor.timer(FREEZE_DEADLINE);
    match futures::future::select(photo, deadline).await {
        futures::future::Either::Left((frozen, _)) => Some(frozen),
        futures::future::Either::Right(_) => None,
    }
}

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
    image_of_owned(frame.clone())
}

/// Lo mismo que [`image_of`] sin copiar los píxeles (el cuadro se entrega): la vista previa del
/// editor arma una imagen por cuadro.
pub(crate) fn image_of_owned(frame: Frame) -> Option<Arc<RenderImage>> {
    let (width, height) = (frame.width(), frame.height());
    let buffer = image::RgbaImage::from_raw(width, height, frame.bgra)?;
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
    /// L: al elegir, la mira se quita y lo elegido se va uniendo mientras se
    /// desplaza la página (`scroll_capture.rs`).
    pub scroll: bool,
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

/// Si la mira abre con «Desplazable» prendido: como quedó la última vez.
fn scroll_file() -> Option<PathBuf> {
    crate::paths::file("capture-scroll.txt")
}

pub fn remembered_scroll() -> bool {
    scroll_file()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .is_some_and(|mode| mode.trim() == "on")
}

fn remember_scroll(scroll: bool) {
    if let Some(path) = scroll_file() {
        let _ = std::fs::write(path, if scroll { "on" } else { "off" });
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
            scroll: false,
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

    /// La pantalla (lógica) en `p`, o toda la foto si no se sabe.
    fn screen_at(&self, p: (f32, f32)) -> Rect {
        self.screens.iter().copied().find(|screen| screen.contains(p, 0.0)).unwrap_or_else(|| self.screen())
    }

    /// El monitor bajo el cursor (o toda la foto si no se sabe).
    fn cursor_screen(&self) -> Rect {
        self.screen_at(self.cursor)
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
    /// Una foto nueva de lo mismo (F, P, el modo texto). Se toma en otro hilo
    /// y se espera a lo más `REFREEZE_WITHIN`: se pide desde el hilo de la
    /// interfaz y, con el equipo saturado, esperarla sin plazo trababa la app
    /// entera con la mira tapando la pantalla.
    fn refreeze(&mut self) -> Result<(Frame, Arc<RenderImage>), String> {
        let started = Instant::now();
        let area = self.frozen.area;
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .name("captura-de-nuevo".into())
            .spawn(move || {
                // `PILL_SLOW_REFREEZE=<ms>` (solo desarrollo): una foto lenta,
                // para probar el plazo.
                #[cfg(debug_assertions)]
                if let Some(ms) = std::env::var("PILL_SLOW_REFREEZE").ok().and_then(|v| v.parse::<u64>().ok()) {
                    std::thread::sleep(std::time::Duration::from_millis(ms));
                }
                let photo = atic_capture::engine::capture_rect(area, include_cursor())
                    .map_err(|error| error.to_string())
                    .and_then(|frame| {
                        let image = image_of(&frame).ok_or(crate::i18n::t("pill.capture.badFrame").to_string())?;
                        Ok((frame, image))
                    });
                let _ = tx.send(photo);
            })
            .map_err(|error| error.to_string())?;
        let (frame, image) = rx
            .recv_timeout(REFREEZE_WITHIN)
            .map_err(|_| crate::i18n::t("pill.capture.slowTitle").to_string())??;
        let old_image = std::mem::replace(&mut self.frozen.image, image);
        let old_frame = std::mem::replace(&mut self.frozen.frame, frame);
        self.frozen.took = started.elapsed();
        Ok((old_frame, old_image))
    }

    /// Los monitores de la foto, para que el modo texto los lea uno por uno
    /// como la Pizarra (`text_select::read_parts`).
    fn text_parts(&self) -> Vec<text_select::Part> {
        let frozen = &self.frozen;
        let all: Vec<PhysRect> = monitors::enumerate().iter().map(|monitor| monitor.bounds).collect();
        monitor_parts(frozen.area, frozen.density(), (frozen.frame.width(), frozen.frame.height()), &all)
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
                let rect = snap_to_screens(Rect::new(x0, y0, x1 - x0, y1 - y0), &self.screens);
                // Grabar es de una pantalla: el arrastre se queda en la que empezó.
                let rect = if self.record { clip_to_screen(rect, self.screen_at(start)) } else { rect };
                return Pick::Region(rect);
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
    ///
    /// Al grabar es **una sola pantalla**: el video sale de un monitor y el mouse se
    /// mide sobre la misma zona, así que una zona que cruce a la otra pantalla
    /// desincroniza el cursor. La pantalla entera es la del cursor, y una zona se
    /// queda en la pantalla donde empezó el arrastre.
    pub fn pick_rect(&self, pick: &Pick) -> Rect {
        if self.record {
            return match pick {
                Pick::Screen => self.cursor_screen(),
                Pick::Region(rect) | Pick::Window(rect, _) => clip_to_screen(*rect, self.screen_at(self.press.unwrap_or(self.cursor))),
            };
        }
        match pick {
            Pick::Region(rect) | Pick::Window(rect, _) => *rect,
            Pick::Screen => self.screen(),
        }
    }

    /// La ventana elegida, si otra de más al frente la tapa en parte. Entonces
    /// se fotografía ella sola (`PrintWindow` en Windows,
    /// `CGWindowListCreateImage` en macOS) y no la pantalla.
    fn covered_window(&self, pick: &Pick) -> Option<isize> {
        let Pick::Window(rect, _) = pick else {
            return None;
        };
        let windows = &self.frozen.windows;
        let at = windows.iter().position(|window| window.rect == *rect)?;
        windows[..at]
            .iter()
            .any(|front| crate::geometry::overlaps(&front.rect, rect))
            .then_some(windows[at].hwnd)
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
/// Lo de `rect` que cabe en `screen` (al menos un píxel lógico).
fn clip_to_screen(rect: Rect, screen: Rect) -> Rect {
    let x0 = rect.x.max(screen.x);
    let y0 = rect.y.max(screen.y);
    let x1 = rect.right().min(screen.right());
    let y1 = rect.bottom().min(screen.bottom());
    Rect::new(x0, y0, (x1 - x0).max(1.0), (y1 - y0).max(1.0))
}

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

/// Cada monitor que toca `area`, en píxeles del cuadro (`k` por unidad de
/// pantalla, `size` el cuadro). Con uno solo, nada: se lee la foto entera.
fn monitor_parts(area: PhysRect, k: f32, size: (u32, u32), monitors: &[PhysRect]) -> Vec<text_select::Part> {
    let to_pixels = |value: i32, max: u32| ((value as f32 * k).round().max(0.0) as u32).min(max);
    let parts: Vec<text_select::Part> = monitors
        .iter()
        .filter_map(|monitor| monitor.intersection(&area))
        .filter_map(|inside| {
            let (left, top) = (to_pixels(inside.x - area.x, size.0), to_pixels(inside.y - area.y, size.1));
            let (right, bottom) = (to_pixels(inside.right() - area.x, size.0), to_pixels(inside.bottom() - area.y, size.1));
            (right > left && bottom > top).then_some((left, top, right - left, bottom - top))
        })
        .collect();
    if parts.len() < 2 {
        return Vec::new();
    }
    parts
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
    [Cancel, WholeScreen, ToggleLive, ToggleDraw, TogglePill, ToggleMic, ToggleSystem, ToggleText, ToggleRecord, ToggleScroll, SelectAllText, CopyText]
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
        // R: Captura o Grabar, lo mismo que el selector de la barra.
        KeyBinding::new("r", ToggleRecord, context),
        // G (como en la Pizarra) o T: elegir texto sobre la foto (`text_select.rs`).
        KeyBinding::new("g", ToggleText, context),
        KeyBinding::new("t", ToggleText, context),
        // L: al elegir, captura desplazable (`scroll_capture.rs`).
        KeyBinding::new("l", ToggleScroll, context),
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
    /// Empezó la captura desplazable: la mira sigue abierta, pero ya no tapa
    /// la pantalla (las ventanas espejo se cierran).
    Scrolling,
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
    /// El cursor está sobre la barra: sin cruz ni lupa.
    over_bar: bool,
    /// M3, modo grabar: el menú de fuentes del split button, abierto.
    sources_open: bool,
    /// El cursor está sobre ese menú (que sale de la barra): sin cruz ni lupa.
    over_menu: bool,
    /// La captura desplazable en curso: la mira ya no se ve.
    scrolling: Option<Scrolling>,
    /// El tamaño lógico de la ventana del overlay (la desplazable va solo en ella).
    viewport: (f32, f32),
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
            over_bar: false,
            sources_open: false,
            over_menu: false,
            scrolling: None,
            viewport: (0.0, 0.0),
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
                if view.text_mode || view.scrolling.is_some() {
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
            None => self.text = Some(text_select::start_parts(&self.session.frozen.frame, self.session.text_parts(), cx)),
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
        // L: lo elegido se va uniendo mientras se desplaza la página. «Toda la
        // pantalla» es la del cursor.
        if self.session.scroll && !self.session.record {
            let rect = match pick {
                Pick::Screen => self.session.cursor_screen(),
                ref other => self.session.pick_rect(other),
            };
            if self.start_scroll(rect, cx) {
                return;
            }
        }
        let rect = self.session.pick_rect(&pick);
        let region = self.session.physical(&rect);
        if self.session.record {
            self.start_countdown(rect, region, cx);
            return;
        }
        // Una ventana tapada por otra sale entera y sin la de enfrente; si no
        // se puede imprimir, se recorta de la pantalla.
        #[cfg(windows)]
        let printed = self.session.covered_window(&pick).and_then(|hwnd| {
            atic_capture::engine::print_window_visual(hwnd)
                .ok()
                .flatten()
                .and_then(|frame| frame.crop(region))
                .map(|frame| (frame, region))
        });
        // En macOS la imagen llega a la escala de su pantalla: va entera, como
        // la foto en vivo (con pantallas mezcladas, `region` es de otra grilla).
        #[cfg(target_os = "macos")]
        let printed = self.session.covered_window(&pick).and_then(|hwnd| {
            atic_capture::engine::print_window(hwnd).ok().flatten().map(|frame| {
                let bounds = frame.bounds;
                (frame, bounds)
            })
        });
        #[cfg(not(any(windows, target_os = "macos")))]
        let printed: Option<(Frame, PhysRect)> = None;
        // En vivo la foto es de ahora: el overlay está excluido de las
        // capturas, así que no hace falta esconderlo antes.
        let (frame, region) = if let Some(printed) = printed {
            printed
        } else if self.session.live {
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

    /// El selector de la barra (o R): Captura ⇄ Grabar.
    fn set_mode(&mut self, record: bool, cx: &mut Context<Self>) {
        if self.session.countdown.is_some() || self.session.record == record {
            return;
        }
        if record {
            self.session.enter_record();
        } else {
            self.session.record = false;
        }
        self.close_sources(cx);
        cx.notify();
    }

    /// Cierra el menú de fuentes de Grabar (M3).
    fn close_sources(&mut self, cx: &mut Context<Self>) {
        if self.sources_open || self.over_menu {
            self.sources_open = false;
            self.over_menu = false;
            cx.notify();
        }
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
        if self.scrolling.is_some() {
            self.cancel_scroll(cx);
            return;
        }
        // Con el menú de fuentes abierto, Esc solo lo cierra.
        if self.sources_open {
            self.close_sources(cx);
            return;
        }
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
        // En la desplazable, Enter es «Listo».
        if self.scrolling.is_some() {
            self.finish_scroll(cx);
            return;
        }
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
    /// M3 Expressive: el esquema de la pill (borde y manijas en `primary`).
    scheme: Option<gpui_m3::Scheme>,
}

/// El lado de las manijas de las esquinas de lo elegido (M3).
const HANDLE: f32 = 10.0;

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
                let edge: Hsla = match self.scheme {
                    Some(s) if self.record => s.error,
                    Some(s) if self.window_hint => s.tertiary,
                    Some(s) => s.primary,
                    None if self.record => rgb(RECORD_EDGE).into(),
                    None if self.window_hint => rgb(0x7aa2ff).into(),
                    None => gpui::white().opacity(0.95),
                };
                window.paint_quad(outline(self.b(&r), edge, BorderStyle::Solid));
                // M3: una región dibujada a mano lleva manijas en las esquinas.
                if self.scheme.is_some() && !self.window_hint {
                    let half = HANDLE / 2.0;
                    for (x, y) in [(r.x, r.y), (r.right(), r.y), (r.x, r.bottom()), (r.right(), r.bottom())] {
                        let handle = self.b(&Rect::new(x - half, y - half, HANDLE, HANDLE));
                        window.paint_quad(fill(handle, edge).corner_radii(gpui::Corners::all(px(3.))));
                    }
                }
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
        // M3: la lupa en un marco `surface_container` redondeado.
        let (frame, rim, center_edge) = match self.scheme {
            Some(s) => (s.surface_container, s.outline_variant, s.primary),
            None => (gpui::black().opacity(0.85), gpui::white().opacity(0.6), gpui::white()),
        };
        let pad = if self.scheme.is_some() { 4.0 } else { 2.0 };
        window.paint_quad(
            fill(self.b(&Rect::new(lx - pad, ly - pad, side + pad * 2.0, side + pad * 2.0)), frame)
                .corner_radii(gpui::Corners::all(px(if self.scheme.is_some() { 8. } else { 0. }))),
        );
        for (i, color) in self.loupe.iter().enumerate() {
            let col = (i as i32 % LOUPE_PIXELS) as f32;
            let row = (i as i32 / LOUPE_PIXELS) as f32;
            let cell = Rect::new(lx + col * LOUPE_CELL, ly + row * LOUPE_CELL, LOUPE_CELL, LOUPE_CELL);
            window.paint_quad(fill(self.b(&cell), *color));
        }
        let center = (LOUPE_PIXELS / 2) as f32 * LOUPE_CELL;
        window.paint_quad(outline(
            self.b(&Rect::new(lx + center, ly + center, LOUPE_CELL, LOUPE_CELL)),
            center_edge,
            BorderStyle::Solid,
        ));
        window.paint_quad(outline(
            self.b(&Rect::new(lx, ly, side, side)),
            rim,
            BorderStyle::Solid,
        ));
    }
}

/// La ayuda de arriba de la pantalla: en M3, un tooltip rico (`m3_hint`).
fn hint(text: impl Into<SharedString>) -> gpui::Div {
    match crate::board::Colors::current().scheme {
        Some(s) => crate::hover::m3_hint(&s, text),
        None => chip(text),
    }
}

/// Etiqueta oscura y redonda sobre la pantalla congelada. En M3 Expressive,
/// un chip de M3 con el esquema de la pill.
fn chip(text: impl Into<SharedString>) -> gpui::Div {
    if let Some(s) = crate::board::Colors::current().scheme {
        return crate::hover::m3_chip(&s, text);
    }
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
        let viewport = window.viewport_size();
        self.viewport = (f32::from(viewport.width), f32::from(viewport.height));
        self.render_shifted((0.0, 0.0), window, cx)
    }
}

impl crate::spread::Spread for CaptureView {
    /// Las otras pantallas dibujan lo mismo con la foto corrida. El modo
    /// texto queda solo en la del overlay.
    fn render_at(&mut self, shift: (f32, f32), window: &mut Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        if shift != (0.0, 0.0) && (self.text_mode || self.scrolling.is_some()) {
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
        if self.scrolling.is_some() {
            return self.scroll_view(cx);
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
        let scheme = crate::board::Colors::current().scheme;
        let paint = Paint {
            offset,
            screen,
            selection,
            window_hint: matches!(pick, Pick::Window(..)),
            record: session.record,
            cursor,
            crosshair: session.press.is_none() && !self.over_bar && !self.over_menu,
            loupe: if self.over_bar || self.over_menu { Vec::new() } else { loupe },
            loupe_origin: (lx, ly),
            scheme,
        };

        // Mientras se arrastra, la barra no estorba. En M3 entra con un
        // resorte desde abajo cada vez que vuelve.
        let bar_in = match (session.press.is_none(), scheme) {
            (true, Some(_)) => crate::hover::m3_bar_entrance("capture-bar-in", window, cx),
            _ => 1.0,
        };
        let toolbar = session.press.is_none().then(|| self.toolbar(help, offset, bar_in, cx));
        // Consejo de primera vez: la captura desplazable (`coach::tip`).
        let scroll_tip = if session.scroll {
            crate::coach::tip_done("capture.scroll");
            None
        } else if !session.record && session.press.is_none() && crate::coach::tip("capture.scroll") {
            Some(crate::coach::tip_chip(scheme.as_ref(), Some("L"), crate::i18n::t("pill.capture.tipScroll")))
        } else {
            None
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
            // Desplazable: que se note sobre lo elegido.
            let text = if session.scroll && !session.record {
                format!("{} · {text}", crate::i18n::t("pill.capture.tool.scroll"))
            } else {
                text
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
            .on_action(cx.listener(|view, _: &ToggleDraw, _, cx| view.toggle_draw(cx)))
            .on_action(cx.listener(|view, _: &ToggleScroll, _, cx| view.toggle_scroll(cx)))
            .on_action(cx.listener(|view, _: &ToggleRecord, _, cx| {
                let record = !view.session.record;
                view.set_mode(record, cx);
            }))
            .on_action(cx.listener(Self::toggle_text))
            .on_action(cx.listener(Self::select_all_text))
            .size_full()
            .relative()
            .cursor(CursorStyle::Crosshair)
            .on_mouse_move(cx.listener(move |view, event: &MouseMoveEvent, _, cx| {
                view.session.cursor = view.session.frozen.cursor_in(event.position, shift);
                view.session.request_loupe(cx);
                cx.notify();
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |view, event: &MouseDownEvent, _, cx| {
                    // Un clic fuera del menú de fuentes lo cierra y no elige.
                    if view.sources_open {
                        view.close_sources(cx);
                        cx.stop_propagation();
                        return;
                    }
                    let p = view.session.frozen.cursor_in(event.position, shift);
                    view.session.cursor = p;
                    view.session.press = Some(p);
                    cx.stop_propagation();
                    cx.notify();
                }),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(move |view, event: &MouseUpEvent, _, cx| {
                    view.session.cursor = view.session.frozen.cursor_in(event.position, shift);
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
                    .flex_col()
                    .items_center()
                    .gap(px(8.))
                    .child(hint(if session.record {
                        let yes = |on: bool| if on { crate::i18n::t("pill.capture.yes") } else { crate::i18n::t("pill.capture.no") };
                        let options = session.record_options;
                        crate::i18n::tf(
                            "pill.capture.recordBar",
                            &[("mic", &yes(options.mic)), ("system", &yes(options.system)), ("pill", &yes(options.show_pill))],
                        )
                    } else if session.scroll {
                        // Desplazable: la ayuda dice qué hacer ahora.
                        crate::i18n::t("pill.capture.scrollOn").to_string()
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
                    }))
                    .children(scroll_tip),
            )
            .children(toolbar)
            .into_any_element()
    }
}

/// Lo que hace un botón de la barra.
#[derive(Clone, Copy)]
enum Tool {
    Mode(bool),
    Draw,
    /// Captura desplazable (`scroll_capture.rs`).
    Scroll,
    Text,
    Mic,
    System,
    Pill,
    /// «Cursor suave» (macOS): grabar sin el cursor del sistema y dibujarlo suavizado.
    SmoothCursor,
    /// M3: la mitad angosta del split button de Grabar, que abre las fuentes.
    Sources,
    /// Toda la pantalla: la foto o, en Grabar, la cuenta para grabarla.
    Screen,
    Close,
}

/// Distancia de la barra al borde de abajo de la pantalla: sobre el Dock en
/// Mac (como la de ⌘⇧5) y sobre la barra de tareas en Windows.
#[cfg(target_os = "macos")]
const BAR_BOTTOM: f32 = 96.0;
/// El ancho de la isla de opciones (el de Grabar, la más ancha) y el de la
/// acción principal.
#[cfg(target_os = "macos")]
const OPTIONS_W: f32 = 404.0;
#[cfg(not(target_os = "macos"))]
const OPTIONS_W: f32 = 428.0;
const PRIMARY_W: f32 = 150.0;
/// En M3, la isla de opciones de Captura (Pizarra, Desplazable y Texto, la más ancha): en
/// Grabar las fuentes van en el menú del split button, que le quita su mitad
/// angosta, y la barra entera mide lo mismo en los dos modos.
const M3_OPTIONS_W: f32 = 372.0;
#[cfg(not(target_os = "macos"))]
const BAR_BOTTOM: f32 = 64.0;

impl CaptureView {
    /// La barra de la mira, abajo al centro de la pantalla del cursor, hecha
    /// con las mismas islas que la de la Pizarra (`board::island`): el modo
    /// (Captura o Grabar, con la píldora que se desliza), lo de cada modo y la
    /// acción principal. La ventana de la mira está excluida de las capturas:
    /// la barra no sale en la foto ni en el video (y al grabar ya se cerró).
    ///
    /// Con varias pantallas cada ventana dibuja su copia (corrida, fuera de
    /// vista en las otras): el indicador va con una clave por ventana para
    /// que una sola lo mueva en cada una.
    ///
    /// En M3 Expressive cada isla es una barra flotante de M3: el modo es un
    /// grupo de botones conectado, las opciones son botones de alternar y la
    /// acción principal va llena (en `error` al grabar). `bar_in` es su
    /// entrada (`m3_bar_entrance`).
    fn toolbar(&self, help: Rect, offset: (f32, f32), bar_in: f32, cx: &mut Context<Self>) -> gpui::AnyElement {
        use crate::board::{island, BUTTON, INK};
        use crate::hover::M3Tone;
        let session = &self.session;
        let t = crate::i18n::t;
        let text: Hsla = rgb(INK).into();
        let colors = crate::board::Colors::current();
        let side = colors.button();
        let bar_of = |id: &'static str| match colors.scheme {
            Some(s) => crate::hover::m3_toolbar(id, &s),
            None => island(id),
        };
        let red: Hsla = rgb(RECORD_EDGE).into();
        let record = session.record;
        let options = session.record_options;
        let glide = crate::glide::named(format!("capture-mode-{:.0}-{:.0}", offset.0, offset.1));

        let button = |id: (&'static str, usize),
                      tool: Tool,
                      icon: &'static str,
                      label: Option<&'static str>,
                      tip: &'static str,
                      on: bool,
                      icon_ink: Option<Hsla>,
                      cx: &mut Context<Self>| {
            let el = div()
                .id(id)
                .h(px(side))
                .min_w(px(side))
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .gap(px(6.))
                .when(label.is_some(), |el| el.pl(px(10.)).pr(px(12.)))
                .rounded(px(side / 2.0))
                .cursor_pointer()
                .text_size(px(12.))
                .font_family(crate::fonts::UI)
                .tooltip(crate::hover::tool_tip(tip))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |view, _: &MouseDownEvent, window, cx| {
                        cx.stop_propagation();
                        view.use_tool(tool, window, cx);
                    }),
                )
                .when(matches!(tool, Tool::Mode(_)), |el| {
                    el.child(crate::glide::mark(glide.clone(), matches!(tool, Tool::Mode(true)) as u64))
                });
            if let Some(s) = colors.scheme {
                return el.fx_cx(SharedString::from(format!("{}-{}-fx", id.0, id.1)), move |el, h, window, cx| {
                    let (el, fg) = match tool {
                        // El modo: un grupo conectado; Grabar elegido, en `error_container`.
                        Tool::Mode(record) => {
                            let chosen = on.then_some(if record {
                                (s.error_container, s.on_error_container)
                            } else {
                                (s.secondary_container, s.on_secondary_container)
                            });
                            crate::hover::m3_connected(el, h, (record as usize, 2), chosen, side, &s, window, cx)
                        }
                        _ => {
                            let tone = if on { M3Tone::Selected } else { M3Tone::Standard };
                            crate::hover::m3_tool(el, h, tone, side, &s, window, cx)
                        }
                    };
                    el.font_weight(gpui::FontWeight::MEDIUM)
                        .child(svg().path(icon).flex_none().size(px(18.)).text_color(fg))
                        .when_some(label, |el, label| el.child(label))
                });
            }
            el.fx(SharedString::from(format!("{}-{}-fx", id.0, id.1)), move |el, h| {
                    // Apagado: el gris de la Pizarra, que se lee igual.
                    let fg = if on { text } else { h.mix(text.opacity(0.62), text) };
                    // El modo elegido lo pinta la píldora que se desliza.
                    let bg = if matches!(tool, Tool::Mode(_)) && on {
                        text.opacity(0.0)
                    } else {
                        h.mix(text.opacity(0.0), text.opacity(0.08 + 0.05 * h.press))
                    };
                    el.bg(bg)
                        .text_color(fg)
                        .child(svg().path(icon).flex_none().size(px(17. - 1.5 * h.press)).text_color(icon_ink.unwrap_or(fg)))
                        .when_some(label, |el, label| el.child(label))
                })
        };

        let mode = bar_of("capture-mode")
            .relative()
            .when(colors.scheme.is_none(), |el| {
                el.child(crate::glide::indicator(
                    glide.clone(),
                    Some(record as u64),
                    crate::glide::Look { fill: text.opacity(0.14), radius: BUTTON / 2.0, edge: None },
                ))
            })
            .child(button(("capture-mode", 0), Tool::Mode(false), "icons/scan.svg", Some(t("pill.capture.tool.capture")), t("pill.capture.tip.capture"), !record, None, cx))
            .child(button(
                ("capture-mode", 1),
                Tool::Mode(true),
                "icons/video.svg",
                Some(t("pill.capture.tool.record")),
                t("pill.capture.tip.record"),
                record,
                // El rojo de grabar: que se note en qué modo estás.
                record.then_some(red),
                cx,
            ));

        // M3, Grabar: la acción es un split button y su mitad angosta abre las
        // fuentes (micrófono, sonido, la pill), que salen de la isla.
        let split = record && colors.scheme.is_some();
        // Anchos fijos: al cambiar de modo la barra no se corre y el selector
        // queda bajo el cursor.
        let options_w = match colors.scheme {
            None => OPTIONS_W,
            Some(_) if split => M3_OPTIONS_W - side - 2.0,
            Some(_) => M3_OPTIONS_W,
        };
        let mut options_island = bar_of("capture-options").w(px(options_w)).justify_center();
        if record && !split {
            options_island = options_island
                .child(button(
                    ("capture-opt", 0),
                    Tool::Mic,
                    if options.mic { "icons/mic.svg" } else { "icons/mic-off.svg" },
                    Some(t("pill.capture.tool.mic")),
                    t("pill.capture.tip.mic"),
                    options.mic,
                    None,
                    cx,
                ))
                .child(button(
                    ("capture-opt", 1),
                    Tool::System,
                    if options.system { "icons/volume-2.svg" } else { "icons/volume-x.svg" },
                    Some(t("pill.capture.tool.system")),
                    t("pill.capture.tip.system"),
                    options.system,
                    None,
                    cx,
                ))
                .child(button(
                    ("capture-opt", 2),
                    Tool::Pill,
                    "icons/layers.svg",
                    Some(t("pill.capture.tool.pill")),
                    t("pill.capture.tip.pill"),
                    options.show_pill,
                    None,
                    cx,
                ));
        }
        if record {
            options_island = options_island.child(button(
                ("capture-opt", 5),
                Tool::SmoothCursor,
                "icons/mouse-pointer-2.svg",
                Some(t("pill.capture.tool.smoothCursor")),
                t("pill.capture.tip.smoothCursor"),
                options.smooth_cursor,
                None,
                cx,
            ));
        } else {
            options_island = options_island
                .child(button(
                    ("capture-opt", 3),
                    Tool::Draw,
                    "icons/pencil.svg",
                    Some(t("pill.capture.tool.draw")),
                    t("pill.capture.tip.draw"),
                    session.draw,
                    None,
                    cx,
                ))
                .child(button(
                    ("capture-opt", 6),
                    Tool::Scroll,
                    "icons/arrow-down.svg",
                    Some(t("pill.capture.tool.scroll")),
                    t("pill.capture.tip.scroll"),
                    session.scroll,
                    None,
                    cx,
                ))
                .child(button(("capture-opt", 4), Tool::Text, "icons/scan-text.svg", Some(t("pill.capture.tool.text")), t("pill.capture.tip.text"), false, None, cx));
        }

        // La acción principal lleva su nombre, como «Listo» en la Pizarra; en
        // Grabar es roja, para que se vea cómo empezar.
        let primary = div()
            .id("capture-go")
            .h(px(side))
            .min_w(px(PRIMARY_W))
            .justify_center()
            .pl(px(10.))
            .pr(px(13.))
            .flex()
            .flex_none()
            .items_center()
            .gap(px(7.))
            .rounded(px(side / 2.0))
            .cursor_pointer()
            .text_size(px(12.))
            .font_family(crate::fonts::UI)
            .tooltip(crate::hover::tool_tip(t("pill.capture.tip.screen")))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|view, _: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    view.use_tool(Tool::Screen, window, cx);
                }),
            )
            .fx_cx("capture-go-fx", move |el, h, window, cx| {
                // M3: lleno en `primary`, o en `error` para grabar; el punto
                // de grabar se cuadra (como el botón de detener) al apuntarlo.
                if let Some(s) = colors.scheme {
                    let (el, fg) = if split {
                        crate::hover::m3_split(el, h, false, false, (s.error, s.on_error), side, window, cx)
                    } else {
                        let tone = if record { M3Tone::Error } else { M3Tone::Filled };
                        crate::hover::m3_tool(el, h, tone, side, &s, window, cx)
                    };
                    let el = el.font_weight(gpui::FontWeight::MEDIUM).text_size(px(13.));
                    return if record {
                        let dot = gpui_m3::motion::animate_f32(
                            "capture-go-dot",
                            gpui_m3::Spring::SPATIAL_FAST,
                            if h.t > 0.5 { 2.5 } else { 6.0 },
                            window,
                            cx,
                        );
                        el.child(div().size(px(12.)).rounded(px(dot.max(0.0))).bg(fg)).child(t("pill.capture.tool.recordScreen"))
                    } else {
                        el.child(svg().path("icons/monitor.svg").size(px(18.)).text_color(fg)).child(t("pill.capture.tool.captureScreen"))
                    };
                }
                if record {
                    el.bg(red.opacity(0.85 + 0.15 * h.t - 0.1 * h.press))
                        .text_color(gpui::white())
                        .child(div().size(px(10.)).rounded_full().bg(gpui::white()))
                        .child(t("pill.capture.tool.recordScreen"))
                } else {
                    el.bg(text.opacity(0.14 + 0.06 * h.t + 0.04 * h.press))
                        .text_color(text)
                        .child(svg().path("icons/monitor.svg").size(px(16.)).text_color(text))
                        .child(t("pill.capture.tool.captureScreen"))
                }
            });
        let divider = match colors.scheme {
            Some(s) => crate::hover::m3_toolbar_divider(&s),
            None => div().flex_none().w(px(1.)).h(px(20.)).mx(px(4.)).bg(gpui::white().opacity(0.10)),
        };
        let primary = match colors.scheme {
            Some(s) if split => {
                // La mitad angosta: el chevron gira y, con el menú abierto, se
                // vuelve tonal (`error_container`).
                let open = self.sources_open;
                let chevron = div()
                    .id("capture-sources")
                    .h(px(side))
                    .w(px(side))
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .when(!open, |el| el.tooltip(crate::hover::tool_tip(t("pill.capture.tip.sources"))))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|view, _: &MouseDownEvent, window, cx| {
                            cx.stop_propagation();
                            view.use_tool(Tool::Sources, window, cx);
                        }),
                    )
                    .fx_cx("capture-sources-fx", move |el, h, window, cx| {
                        let pair = if open { (s.error_container, s.on_error_container) } else { (s.error, s.on_error) };
                        let (el, fg) = crate::hover::m3_split(el, h, true, open, pair, side, window, cx);
                        let turn = gpui_m3::motion::animate_f32(
                            "capture-sources-turn",
                            gpui_m3::Spring::SPATIAL_FAST,
                            if open { 0.5 } else { 0.0 },
                            window,
                            cx,
                        );
                        el.child(
                            svg()
                                .path("icons/chevron-up.svg")
                                .flex_none()
                                .size(px(18.))
                                .text_color(fg)
                                .with_transformation(gpui::Transformation::rotate(gpui::percentage(turn))),
                        )
                    });
                let menu = open.then(|| self.sources_menu(s, cx));
                div()
                    .relative()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(2.))
                    .child(primary)
                    .child(chevron)
                    // El menú sale hacia arriba, alineado a la derecha del botón.
                    .when_some(menu, |el, menu| {
                        el.child(
                            div().absolute().right(px(0.)).bottom(px(side + 12.)).child(
                                gpui::deferred(
                                    gpui::anchored()
                                        .anchor(gpui::Corner::BottomRight)
                                        .snap_to_window_with_margin(px(8.))
                                        .child(menu),
                                )
                                .with_priority(1),
                            ),
                        )
                    })
                    .into_any_element()
            }
            _ => primary.into_any_element(),
        };
        let go = bar_of("capture-go-island")
            .child(primary)
            .child(divider)
            .child(button(("capture-close", 0), Tool::Close, "icons/x.svg", None, t("pill.capture.tip.close"), false, None, cx));

        let bar = div()
            .id("capture-toolbar")
            .flex()
            .items_center()
            .gap(px(8.))
            .cursor(CursorStyle::Arrow)
            .on_hover(cx.listener(|view, hovered: &bool, _, cx| {
                if view.over_bar != *hovered {
                    view.over_bar = *hovered;
                    cx.notify();
                }
            }))
            // Ni elegir ni soltar sobre la barra: es solo de botones.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_up(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(mode)
            .child(options_island)
            .child(go);
        div()
            .absolute()
            .left(px(help.x + offset.0))
            // M3: lo que le falta por entrar, corrido hacia abajo.
            .top(px(help.bottom() - BAR_BOTTOM - side - 8.0 + offset.1 + 16.0 * (1.0 - bar_in)))
            .opacity((bar_in * 1.5).clamp(0.0, 1.0))
            .w(px(help.w))
            .flex()
            .justify_center()
            .child(bar)
            .into_any_element()
    }

    /// M3: el menú de fuentes de Grabar, con lo que entra al video. Cada
    /// fuente se prende o apaga sin cerrarlo; elegida, en `secondary_container`.
    fn sources_menu(&self, s: gpui_m3::Scheme, cx: &mut Context<Self>) -> gpui::AnyElement {
        let t = crate::i18n::t;
        let options = self.session.record_options;
        let item = |index: usize, tool: Tool, icon: &'static str, name: &'static str, key: &'static str, on: bool, cx: &mut Context<Self>| {
            let (base, fg) = if on {
                (s.secondary_container, s.on_secondary_container)
            } else {
                (s.on_surface.opacity(0.0), s.on_surface)
            };
            let sub = if on { s.on_secondary_container } else { s.on_surface_variant };
            div()
                .id(("capture-source", index))
                .h(px(44.))
                .w_full()
                .px(px(12.))
                .flex()
                .items_center()
                .gap(px(12.))
                // Concéntrica con el menú (16 menos su margen de 5).
                .rounded(px(11.))
                .cursor_pointer()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |view, _: &MouseDownEvent, window, cx| {
                        cx.stop_propagation();
                        view.use_tool(tool, window, cx);
                    }),
                )
                .child(svg().path(icon).flex_none().size(px(18.)).text_color(if on { fg } else { s.on_surface_variant }))
                .child(div().flex_1().min_w_0().text_size(px(13.)).font_weight(gpui::FontWeight::MEDIUM).text_color(fg).child(name))
                .child(
                    div()
                        .flex_none()
                        .px(px(6.))
                        .py(px(1.))
                        .rounded(gpui_m3::corner::XS)
                        .border_1()
                        .border_color(s.outline_variant)
                        .text_size(px(10.5))
                        .text_color(sub)
                        .child(key),
                )
                .fx(SharedString::from(format!("capture-source-{index}-fx")), move |el, h| {
                    el.bg(crate::hover::m3_layer(base, fg, h))
                })
        };
        crate::hover::m3_menu("capture-sources-menu", &s)
            .min_w(px(220.))
            .p(px(5.))
            .flex()
            .flex_col()
            .gap(px(2.))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_hover(cx.listener(|view, hovered: &bool, _, cx| {
                if view.over_menu != *hovered {
                    view.over_menu = *hovered;
                    cx.notify();
                }
            }))
            .child(item(
                0,
                Tool::Mic,
                if options.mic { "icons/mic.svg" } else { "icons/mic-off.svg" },
                t("pill.capture.tool.mic"),
                "M",
                options.mic,
                cx,
            ))
            .child(item(
                1,
                Tool::System,
                if options.system { "icons/volume-2.svg" } else { "icons/volume-x.svg" },
                t("pill.capture.tool.system"),
                "A",
                options.system,
                cx,
            ))
            .child(item(2, Tool::Pill, "icons/layers.svg", t("pill.capture.tool.pill"), "P", options.show_pill, cx))
            .into_any_element()
    }

    fn use_tool(&mut self, tool: Tool, window: &mut Window, cx: &mut Context<Self>) {
        // Las fuentes se eligen con el menú abierto; lo demás lo cierra.
        if !matches!(tool, Tool::Mic | Tool::System | Tool::Pill | Tool::Sources) {
            self.close_sources(cx);
        }
        match tool {
            Tool::Mode(record) => self.set_mode(record, cx),
            Tool::Draw => self.toggle_draw(cx),
            Tool::Scroll => self.toggle_scroll(cx),
            Tool::Text => {
                self.over_bar = false;
                self.enter_text(cx);
            }
            Tool::Mic => self.toggle_option(|options| options.mic = !options.mic, cx),
            Tool::System => self.toggle_option(|options| options.system = !options.system, cx),
            Tool::Pill => self.toggle_pill(&TogglePill, window, cx),
            Tool::SmoothCursor => self.toggle_option(|options| options.smooth_cursor = !options.smooth_cursor, cx),
            Tool::Sources => {
                if self.sources_open {
                    self.close_sources(cx);
                } else {
                    self.sources_open = true;
                    cx.notify();
                }
            }
            Tool::Screen => {
                self.over_bar = false;
                self.session.press = None;
                let draw = self.session.draw;
                self.choose(Pick::Screen, draw, cx);
            }
            Tool::Close => cx.emit(CaptureEvent::Cancelled),
        }
    }

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
            crate::keymap::hint_owned(crate::i18n::tf(
                "pill.capture.textHint",
                &[("primary", &crate::keymap::primary_label())],
            ))
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
            // Mantener Shift (la palabra) o Ctrl/⌘ (todo).
            .on_modifiers_changed(cx.listener(|view, event: &gpui::ModifiersChangedEvent, window, cx| {
                let map = view.text_map();
                text_select::modifiers_changed(view, map, &event.modifiers, window, cx);
            }))
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
                    .child(self::hint(hint)),
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
            scheme: crate::board::Colors::current().scheme,
        };
        // M3: el número dentro de una galleta en `error_container`, el momento
        // héroe antes de grabar. Quieta: la cuenta ya redibuja cada segundo.
        let count: gpui::AnyElement = match paint.scheme {
            Some(s) => gpui_m3::Shape::new(gpui_m3::ShapeName::Cookie12)
                .size(px(COUNTDOWN_SIZE))
                .color(s.error_container)
                .child(
                    div()
                        .text_color(s.on_error_container)
                        .text_size(px(COUNTDOWN_SIZE * 0.5))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .font_family(crate::fonts::UI)
                        .child(number.to_string()),
                )
                .into_any_element(),
            None => div()
                .size(px(COUNTDOWN_SIZE))
                .rounded_full()
                .bg(gpui::black().opacity(0.6))
                .flex()
                .items_center()
                .justify_center()
                .text_color(gpui::white())
                .text_size(px(COUNTDOWN_SIZE * 0.55))
                .font_family(crate::fonts::UI)
                .child(number.to_string())
                .into_any_element(),
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
                    .child(count),
            )
            .child(
                div()
                    .absolute()
                    .left(px(help.x + offset.0))
                    .top(px(help.y + 10.0 + offset.1))
                    .w(px(help.w))
                    .flex()
                    .justify_center()
                    .child(hint(crate::i18n::t("pill.capture.recordSoon"))),
            )
            .into_any_element()
    }
}

// --- La captura desplazable -------------------------------------------------
//
// Con L (o «Desplazable» en la barra) lo elegido no se fotografía al tiro: la
// mira deja de tapar la pantalla, queda un marco alrededor y una barra con
// «Listo» y «Cancelar», y la ventana deja pasar el mouse (salvo sobre la
// barra) para que la rueda desplace la página. Mientras, en segundo plano se
// toman cuadros de la región y se unen (`scroll_capture.rs`). El overlay está
// excluido de las capturas: el marco y la barra no salen en los cuadros.

/// La pausa entre un cuadro y el siguiente. Corta: mientras menos se desplace
/// la página entre dos cuadros, más comparten y mejor calzan.
const SCROLL_EVERY: std::time::Duration = std::time::Duration::from_millis(50);
/// El alto máximo de lo unido, en píxeles del cuadro (con 1600 de ancho, ~130 MB).
const SCROLL_MAX: u32 = 20_000;
/// La barra de la desplazable y la miniatura, en lógico.
const SCROLL_BAR_W: f32 = 440.0;
const SCROLL_THUMB_W: f32 = 88.0;
const SCROLL_GAP: f32 = 12.0;
/// El grosor del marco, por fuera de lo elegido.
const SCROLL_EDGE: f32 = 2.0;
/// La barra de desplazamiento de la página, a la derecha: se mueve con ella y
/// no cuenta al calzar los cuadros (en lógico).
const SCROLLBAR_SKIP: f32 = 18.0;
/// Cada cuántos cuadros va un resumen al log.
const SCROLL_LOG_EVERY: u32 = 25;

/// La captura desplazable en curso.
struct Scrolling {
    /// Lo elegido, en coordenadas de la foto.
    rect: Rect,
    /// Lo mismo en coordenadas de pantalla: lo que se fotografía.
    region: PhysRect,
    /// La barra, en coordenadas de la foto: con el cursor encima la ventana
    /// recibe el mouse.
    bar: Rect,
    /// El hueco de la miniatura junto a lo elegido, si cabe.
    thumb: Option<Rect>,
    /// Lo que la miniatura puede medir, en píxeles.
    thumb_pixels: (u32, u32),
    /// La unión. `None` mientras un cuadro se procesa en segundo plano.
    stitcher: Option<Stitcher>,
    /// La miniatura de lo unido y su tamaño lógico.
    preview: Option<(Arc<RenderImage>, (f32, f32))>,
    height: u32,
    /// El último cuadro no calzó con lo anterior.
    lost: bool,
    full: bool,
    /// Se apretó «Listo» con un cuadro en proceso: se termina al volver.
    finish: bool,
    /// Para el log: qué pasó con los cuadros.
    tally: ScrollTally,
}

/// Cuántos cuadros se tomaron y qué pasó con cada uno (para el log).
#[derive(Default, Debug)]
struct ScrollTally {
    frames: u32,
    /// Con filas nuevas, abajo o arriba; `above`: los de arriba.
    added: u32,
    above: u32,
    same: u32,
    /// Desplazados por lo que ya estaba unido.
    seen: u32,
    lost: u32,
    errors: u32,
}

/// Con `PILL_SCROLL_DEBUG` cada cuadro de la desplazable se guarda como PNG,
/// para ver qué se fotografía y repetir la unión
/// (`scroll_capture::tests::cuadros_guardados_se_unen`). `1`: una carpeta
/// nueva en la temporal del sistema; otro valor: esa carpeta.
fn scroll_debug_dir() -> Option<PathBuf> {
    let value = std::env::var("PILL_SCROLL_DEBUG").ok().filter(|value| !value.is_empty() && value != "0")?;
    let dir = if value == "1" {
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
        std::env::temp_dir().join(format!("atic-scroll-{stamp}"))
    } else {
        PathBuf::from(value)
    };
    match std::fs::create_dir_all(&dir) {
        Ok(()) => Some(dir),
        Err(error) => {
            tracing::warn!(%error, carpeta = %dir.display(), "captura desplazable: no se pudo crear la carpeta de depuración");
            None
        }
    }
}

/// Un cuadro de la desplazable, ya procesado en segundo plano.
struct ScrollSample {
    step: Step,
    /// El tamaño del cuadro, en píxeles.
    size: (u32, u32),
    thumb: Option<(u32, u32, Vec<u8>)>,
    error: Option<String>,
    /// Lo que tardó la foto, y la foto más la unión.
    shot: std::time::Duration,
    took: std::time::Duration,
}

impl Default for ScrollSample {
    fn default() -> Self {
        Self {
            step: Step::Lost,
            size: (0, 0),
            thumb: None,
            error: None,
            shot: std::time::Duration::ZERO,
            took: std::time::Duration::ZERO,
        }
    }
}

impl Drop for Scrolling {
    fn drop(&mut self) {
        if let Some((image, _)) = self.preview.take() {
            crate::release::image(image);
        }
    }
}

impl CaptureView {
    /// D: la pizarra al soltar. No va junto con la desplazable.
    fn toggle_draw(&mut self, cx: &mut Context<Self>) {
        self.session.draw = !self.session.draw;
        if self.session.draw && self.session.scroll {
            self.session.scroll = false;
            remember_scroll(false);
        }
        cx.notify();
    }

    /// L: lo próximo que se elija se captura desplazando.
    fn toggle_scroll(&mut self, cx: &mut Context<Self>) {
        if self.session.record || self.text_mode || self.scrolling.is_some() {
            return;
        }
        self.session.scroll = !self.session.scroll;
        if self.session.scroll {
            self.session.draw = false;
        }
        remember_scroll(self.session.scroll);
        tracing::info!(prendido = self.session.scroll, "captura desplazable: modo");
        cx.notify();
    }

    /// Si la ventana debe dejar pasar el mouse: en la desplazable, salvo con
    /// el cursor (lógico de la ventana) sobre la barra.
    pub fn passes_mouse(&self, cursor: Option<(f32, f32)>) -> bool {
        let Some(scrolling) = self.scrolling.as_ref() else {
            return false;
        };
        let offset = self.session.frozen.offset;
        let bar = Rect::new(scrolling.bar.x + offset.0, scrolling.bar.y + offset.1, scrolling.bar.w, scrolling.bar.h);
        !cursor.is_some_and(|cursor| bar.contains(cursor, 4.0))
    }

    /// Empieza a unir lo elegido. `false` si no se puede (la mira sigue y se
    /// captura como siempre): en macOS, con varias pantallas, solo en la de
    /// la pill, que es la que tiene la ventana del overlay.
    fn start_scroll(&mut self, rect: Rect, cx: &mut Context<Self>) -> bool {
        let offset = self.session.frozen.offset;
        let (vw, vh) = self.viewport;
        let inside = rect.x + offset.0 >= -0.5
            && rect.y + offset.1 >= -0.5
            && rect.right() + offset.0 <= vw + 0.5
            && rect.bottom() + offset.1 <= vh + 0.5;
        if rect.w < 16.0 || rect.h < 16.0 || !inside {
            tracing::warn!(
                elegido = ?rect,
                ventana = ?self.viewport,
                desplazamiento = ?offset,
                "captura desplazable: lo elegido no cabe en la pantalla del overlay; va como captura normal"
            );
            return false;
        }
        let session = &self.session;
        let region = session.frozen.to_screen(session.physical(&rect));
        let screen = session.screen_at((rect.x + rect.w / 2.0, rect.y + rect.h / 2.0));
        let bar_h = crate::board::Colors::current().button() + 2.0 * crate::hover::M3_TOOLBAR_PAD;
        let bar_w = SCROLL_BAR_W.min(screen.w - 16.0);
        let bar_x = (rect.x + rect.w / 2.0 - bar_w / 2.0).clamp(screen.x + 8.0, (screen.right() - bar_w - 8.0).max(screen.x + 8.0));
        // Debajo de lo elegido, o encima; si no cabe, adentro, abajo.
        let bar_y = if rect.bottom() + SCROLL_GAP + bar_h + 8.0 <= screen.bottom() {
            rect.bottom() + SCROLL_GAP
        } else if rect.y - SCROLL_GAP - bar_h >= screen.y + 8.0 {
            rect.y - SCROLL_GAP - bar_h
        } else {
            rect.bottom() - SCROLL_GAP - bar_h
        };
        let thumb_h = rect.h.min(screen.bottom() - 8.0 - rect.y).max(48.0);
        let thumb_x = if rect.right() + SCROLL_GAP + SCROLL_THUMB_W + 8.0 <= screen.right() {
            Some(rect.right() + SCROLL_GAP)
        } else if rect.x - SCROLL_GAP - SCROLL_THUMB_W >= screen.x + 8.0 {
            Some(rect.x - SCROLL_GAP - SCROLL_THUMB_W)
        } else {
            None
        };
        let scale = session.frozen.scale;
        self.scrolling = Some(Scrolling {
            rect,
            region,
            bar: Rect::new(bar_x, bar_y, bar_w, bar_h),
            thumb: thumb_x.map(|x| Rect::new(x, rect.y, SCROLL_THUMB_W, thumb_h)),
            thumb_pixels: ((SCROLL_THUMB_W * scale) as u32, (thumb_h * scale) as u32),
            stitcher: Some(Stitcher::new(SCROLL_MAX, (SCROLLBAR_SKIP * scale).round() as u32)),
            preview: None,
            height: 0,
            lost: false,
            full: false,
            finish: false,
            tally: ScrollTally::default(),
        });
        tracing::info!(
            region = ?region,
            elegido = ?rect,
            escala = scale,
            vivo = session.live,
            "captura desplazable: empieza; desplaza la página bajo el marco"
        );
        let debug = scroll_debug_dir();
        if let Some(dir) = debug.as_ref() {
            tracing::info!(carpeta = %dir.display(), "captura desplazable: los cuadros se guardan (PILL_SCROLL_DEBUG)");
        }
        self.session.press = None;
        self.over_bar = false;
        self.sources_open = false;
        self.over_menu = false;
        cx.emit(CaptureEvent::Scrolling);
        self.sample_scroll(debug, cx);
        cx.notify();
        true
    }

    /// Toma cuadros en segundo plano hasta que la desplazable termine. La
    /// vista se redibuja solo cuando cambia lo que muestra.
    fn sample_scroll(&mut self, debug: Option<PathBuf>, cx: &mut Context<Self>) {
        cx.spawn(async move |view, cx| {
            let mut index = 0u32;
            loop {
                let job = view
                    .update(cx, |view, _| {
                        let scrolling = view.scrolling.as_mut()?;
                        let stitcher = scrolling.stitcher.take()?;
                        Some((stitcher, scrolling.region, scrolling.thumb_pixels))
                    })
                    .ok()
                    .flatten();
                let Some((mut stitcher, region, (thumb_w, thumb_h))) = job else {
                    break;
                };
                let dump = debug.as_ref().map(|dir| dir.join(format!("{index:05}.png")));
                index += 1;
                let (stitcher, sample) = cx
                    .background_spawn(async move {
                        let started = std::time::Instant::now();
                        let mut sample = ScrollSample::default();
                        match atic_capture::engine::capture_rect(region, false) {
                            Ok(frame) => {
                                sample.size = (frame.width(), frame.height());
                                sample.shot = started.elapsed();
                                sample.step = stitcher.push(frame.width(), frame.height(), &frame.bgra);
                                if let Some(path) = dump {
                                    if let Ok(png) = atic_capture::encoding::bgra_to_png(frame.width(), frame.height(), &frame.bgra) {
                                        let _ = std::fs::write(path, png);
                                    }
                                }
                            }
                            Err(error) => sample.error = Some(error.to_string()),
                        }
                        if let Step::First | Step::Added(_) | Step::AddedAbove(_) = sample.step {
                            sample.thumb = stitcher.thumbnail(thumb_w, thumb_h);
                        }
                        sample.took = started.elapsed();
                        (stitcher, sample)
                    })
                    .await;
                let go_on = view.update(cx, |view, cx| view.scroll_step(stitcher, sample, cx)).unwrap_or(false);
                if !go_on {
                    break;
                }
                cx.background_executor().timer(SCROLL_EVERY).await;
            }
        })
        .detach();
    }

    /// Vuelve un cuadro procesado. `false`: no hay que tomar más.
    fn scroll_step(&mut self, stitcher: Stitcher, sample: ScrollSample, cx: &mut Context<Self>) -> bool {
        let scale = self.session.frozen.scale;
        let Some(scrolling) = self.scrolling.as_mut() else {
            return false;
        };
        let ScrollSample { step, size, thumb, error, shot, took } = sample;
        let tally = &mut scrolling.tally;
        tally.frames += 1;
        if tally.frames == 1 {
            // El tamaño del cuadro contra la región dice a qué escala
            // fotografía esa pantalla.
            tracing::info!(
                cuadro = ?size,
                region = ?scrolling.region,
                foto_ms = shot.as_millis() as u64,
                total_ms = took.as_millis() as u64,
                "captura desplazable: primer cuadro"
            );
        }
        // Solo al perder el hilo, no en cada cuadro perdido.
        if error.is_none() && step == Step::Lost && !scrolling.lost {
            tracing::info!(cuadro = tally.frames, alto = stitcher.height(), calce = ?stitcher.detail(), "captura desplazable: no calza");
        }
        match (&error, step) {
            (Some(error), _) => {
                tally.errors += 1;
                // Solo el primero: con el permiso faltando, fallan todos.
                if tally.errors == 1 {
                    tracing::warn!(%error, region = ?scrolling.region, "captura desplazable: no se pudo tomar el cuadro");
                }
            }
            (None, Step::First | Step::Added(_)) => tally.added += 1,
            (None, Step::AddedAbove(_)) => {
                // La primera vez, para ver en el log que se subió de más.
                if tally.above == 0 {
                    tracing::info!(cuadro = tally.frames, alto = stitcher.height(), "captura desplazable: agrega hacia arriba");
                }
                tally.added += 1;
                tally.above += 1;
            }
            (None, Step::Same) => tally.same += 1,
            (None, Step::Seen) => tally.seen += 1,
            (None, Step::Lost) => tally.lost += 1,
            (None, Step::Full) => {}
        }
        if tally.frames % SCROLL_LOG_EVERY == 0 {
            tracing::info!(
                cuadros = tally.frames,
                unidos = tally.added,
                arriba = tally.above,
                iguales = tally.same,
                ya_vistos = tally.seen,
                sin_calzar = tally.lost,
                errores = tally.errors,
                alto = stitcher.height(),
                total_ms = took.as_millis() as u64,
                calce = ?stitcher.detail(),
                "captura desplazable: resumen"
            );
        }
        let before = (scrolling.height, scrolling.lost, scrolling.full);
        scrolling.height = stitcher.height();
        scrolling.full = stitcher.is_full();
        scrolling.stitcher = Some(stitcher);
        match step {
            Step::First | Step::Added(_) | Step::AddedAbove(_) | Step::Same | Step::Seen => scrolling.lost = false,
            Step::Lost => scrolling.lost = true,
            Step::Full => {}
        }
        let mut changed = before != (scrolling.height, scrolling.lost, scrolling.full);
        if let Some((w, h, bgra)) = thumb {
            if let Some(image) = image_of_owned(Frame::new(PhysRect::new(0, 0, w, h), bgra)) {
                if let Some((old, _)) = scrolling.preview.replace((image, (w as f32 / scale, h as f32 / scale))) {
                    crate::release::image(old);
                }
                changed = true;
            }
        }
        let (finish, full) = (scrolling.finish, scrolling.full);
        if finish {
            self.finish_scroll(cx);
            return false;
        }
        if changed {
            cx.notify();
        }
        !full
    }

    /// «Listo» o Enter: lo unido sigue el camino de una captura normal.
    fn finish_scroll(&mut self, cx: &mut Context<Self>) {
        let Some(scrolling) = self.scrolling.as_mut() else {
            return;
        };
        // Un cuadro en proceso: se termina cuando vuelva.
        if scrolling.stitcher.is_none() {
            scrolling.finish = true;
            return;
        }
        let Some(mut scrolling) = self.scrolling.take() else {
            return;
        };
        let rect = scrolling.rect;
        let tally = std::mem::take(&mut scrolling.tally);
        match scrolling.stitcher.take().and_then(Stitcher::into_image) {
            Some((width, height, bgra)) => {
                tracing::info!(
                    ancho = width,
                    alto = height,
                    cuadros = tally.frames,
                    unidos = tally.added,
                    arriba = tally.above,
                    iguales = tally.same,
                    ya_vistos = tally.seen,
                    sin_calzar = tally.lost,
                    errores = tally.errors,
                    "captura desplazable: lista"
                );
                let frame = Frame::new(PhysRect::new(0, 0, width, height), bgra);
                let region = frame.bounds;
                cx.emit(CaptureEvent::Chosen {
                    frame,
                    region,
                    rect,
                    draw: false,
                });
            }
            None => {
                tracing::warn!(?tally, "captura desplazable: terminó sin ningún cuadro");
                cx.emit(CaptureEvent::Cancelled);
            }
        }
    }

    /// «Cancelar» o Esc en la desplazable.
    fn cancel_scroll(&mut self, cx: &mut Context<Self>) {
        if let Some(scrolling) = self.scrolling.as_ref() {
            tracing::info!(alto = scrolling.height, tally = ?scrolling.tally, "captura desplazable: cancelada");
        }
        cx.emit(CaptureEvent::Cancelled);
    }

    /// Lo que se ve mientras tanto: lo de afuera apenas oscuro, el marco, la
    /// miniatura y la barra. Sin la foto: la pantalla es la de verdad.
    fn scroll_view(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let Some(scrolling) = self.scrolling.as_ref() else {
            return div().into_any_element();
        };
        let t = crate::i18n::t;
        let offset = self.session.frozen.offset;
        let screen = self.session.screen();
        let colors = crate::board::Colors::current();
        let ink: Hsla = rgb(crate::board::INK).into();
        let (edge, muted) = match colors.scheme {
            Some(s) => (s.primary, s.on_surface_variant),
            None => (gpui::white().opacity(0.95), ink.opacity(0.72)),
        };
        let at = move |r: Rect| bounds(&Rect::new(r.x + offset.0, r.y + offset.1, r.w, r.h));
        let r = scrolling.rect;
        let e = SCROLL_EDGE;
        let paint = move |window: &mut Window| {
            let shade: Hsla = gpui::black().opacity(DIM * 0.5);
            let s = screen;
            let outer = Rect::new(r.x - e, r.y - e, r.w + 2.0 * e, r.h + 2.0 * e);
            for band in [
                Rect::new(s.x, s.y, s.w, (outer.y - s.y).max(0.0)),
                Rect::new(s.x, outer.bottom(), s.w, (s.bottom() - outer.bottom()).max(0.0)),
                Rect::new(s.x, outer.y, (outer.x - s.x).max(0.0), outer.h),
                Rect::new(outer.right(), outer.y, (s.right() - outer.right()).max(0.0), outer.h),
            ] {
                window.paint_quad(fill(at(band), shade));
            }
            // El marco va por fuera: lo elegido queda entero a la vista.
            for side in [
                Rect::new(outer.x, outer.y, outer.w, e),
                Rect::new(outer.x, r.bottom(), outer.w, e),
                Rect::new(outer.x, r.y, e, r.h),
                Rect::new(r.right(), r.y, e, r.h),
            ] {
                window.paint_quad(fill(at(side), edge));
            }
        };

        // Arriba qué hacer (o qué pasó); abajo cuánto va y cómo terminar.
        let (title, warn) = if scrolling.full {
            (t("pill.capture.scroll.full"), true)
        } else if scrolling.lost {
            (t("pill.capture.scroll.lost"), true)
        } else {
            (t("pill.capture.scroll.start"), false)
        };
        let progress: SharedString = crate::i18n::tf("pill.capture.scroll.height", &[("px", &scrolling.height)]).into();
        let title_ink = match (colors.scheme, warn) {
            (Some(s), true) => s.error,
            (Some(s), false) => s.on_surface,
            (None, _) => ink,
        };

        let thumb = scrolling.thumb.zip(scrolling.preview.clone()).map(|(slot, (image, (w, h)))| {
            let fit = (slot.w / w.max(1.0)).min(slot.h / h.max(1.0)).min(1.0);
            div()
                .absolute()
                .left(px(slot.x + offset.0))
                .top(px(slot.y + offset.1))
                .w(px(w * fit))
                .h(px(h * fit))
                .rounded(px(6.))
                .overflow_hidden()
                .border_1()
                .border_color(edge)
                .child(img(image).size_full())
        });

        let b = scrolling.bar;
        let bar = match colors.scheme {
            Some(s) => crate::hover::m3_toolbar("capture-scroll-bar", &s),
            None => crate::board::island("capture-scroll-bar"),
        }
        .absolute()
        .left(px(b.x + offset.0))
        .top(px(b.y + offset.1))
        .w(px(b.w))
        .h(px(b.h))
        .cursor(CursorStyle::Arrow)
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(
            div()
                .flex_1()
                .min_w_0()
                .pl(px(12.))
                .pr(px(6.))
                .flex()
                .flex_col()
                .font_family(crate::fonts::UI)
                .child(
                    div()
                        .truncate()
                        .text_size(px(12.5))
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(title_ink)
                        .child(title),
                )
                .child(div().truncate().text_size(px(11.)).text_color(muted).child(progress)),
        )
        .child(self.scroll_button("capture-scroll-cancel", "icons/x.svg", None, t("pill.capture.scroll.tipCancel"), false, Self::cancel_scroll, cx))
        .child(self.scroll_button(
            "capture-scroll-done",
            "icons/check.svg",
            Some(t("pill.capture.scroll.done")),
            t("pill.capture.scroll.tipDone"),
            true,
            Self::finish_scroll,
            cx,
        ));

        div()
            .id("capture")
            .track_focus(&self.focus)
            .key_context(KEY_CONTEXT)
            .on_action(cx.listener(Self::cancel))
            .on_action(cx.listener(Self::whole_screen))
            .size_full()
            .relative()
            .child(canvas(|_, _, _| {}, move |_, _, window, _| paint(window)).absolute().size_full())
            .children(thumb)
            .child(bar)
            .into_any_element()
    }

    /// Un botón de la barra de la desplazable, como los de la mira. `filled`:
    /// la acción principal.
    #[allow(clippy::too_many_arguments)]
    fn scroll_button(
        &self,
        id: &'static str,
        icon: &'static str,
        label: Option<&'static str>,
        tip: &'static str,
        filled: bool,
        run: fn(&mut Self, &mut Context<Self>),
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        use crate::hover::M3Tone;
        let colors = crate::board::Colors::current();
        let side = colors.button();
        let text: Hsla = rgb(crate::board::INK).into();
        let el = div()
            .id(id)
            .h(px(side))
            .min_w(px(side))
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .gap(px(6.))
            .when(label.is_some(), |el| el.pl(px(10.)).pr(px(13.)))
            .rounded(px(side / 2.0))
            .cursor_pointer()
            .text_size(px(12.))
            .font_family(crate::fonts::UI)
            .tooltip(crate::hover::tool_tip(tip))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |view, _: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    run(view, cx);
                }),
            );
        let fx = SharedString::from(format!("{id}-fx"));
        if let Some(s) = colors.scheme {
            return el
                .fx_cx(fx, move |el, h, window, cx| {
                    let tone = if filled { M3Tone::Filled } else { M3Tone::Standard };
                    let (el, fg) = crate::hover::m3_tool(el, h, tone, side, &s, window, cx);
                    el.font_weight(gpui::FontWeight::MEDIUM)
                        .child(svg().path(icon).flex_none().size(px(18.)).text_color(fg))
                        .when_some(label, |el, label| el.child(label))
                })
                .into_any_element();
        }
        el.fx(fx, move |el, h| {
            let bg = if filled {
                text.opacity(0.14 + 0.06 * h.t + 0.04 * h.press)
            } else {
                h.mix(text.opacity(0.0), text.opacity(0.08 + 0.05 * h.press))
            };
            el.bg(bg)
                .text_color(text)
                .child(svg().path(icon).flex_none().size(px(17.)).text_color(text))
                .when_some(label, |el, label| el.child(label))
        })
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

    /// Empieza a esperar una foto: devuelve el número de esta espera.
    pub(crate) fn begin_pending(&mut self) -> u64 {
        self.capture_pending = true;
        self.pending_gen += 1;
        self.pending_gen
    }

    /// La espera `gen` sigue siendo la de ahora (no se canceló ni venció).
    pub(crate) fn pending_is(&self, gen: u64) -> bool {
        self.capture_pending && self.pending_gen == gen
    }

    /// Deja de esperar la foto y devuelve la pill a como estaba: vuelve a
    /// responder al cursor y a salir en las capturas. `slow`: venció el plazo,
    /// se avisa que el equipo está ocupado (si no, la pill se veía muerta).
    pub(crate) fn abandon_pending(&mut self, slow: bool, cx: &mut Context<Self>) {
        if !self.capture_pending {
            return;
        }
        self.capture_pending = false;
        self.pending_gen += 1;
        self.capture_text = false;
        self.screen_rec.requested = false;
        if let Some(overlay) = self.overlay.as_ref() {
            overlay.exclude_from_capture(false);
        }
        self.end_span();
        if slow {
            tracing::warn!("captura: la foto no llegó en {} s; se cancela", FREEZE_DEADLINE.as_secs());
            crate::platform::tray_icon::notify(
                crate::i18n::t("pill.capture.slowTitle"),
                crate::i18n::t("pill.capture.slowBody"),
                true,
                None,
            );
        }
        cx.notify();
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
        // Esperando la foto, el mismo atajo cancela: la pill no queda tomada.
        if self.capture_pending {
            self.abandon_pending(false, cx);
            return;
        }
        let gen = self.begin_pending();
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
            self.abandon_pending(false, cx);
            return;
        };
        cx.spawn_in(window, async move |this, cx| {
            if let Some(settle) = settle {
                cx.background_executor().timer(settle).await;
            }
            let Some(frozen) = freeze_in_time(cx.background_executor().clone(), scale, area).await else {
                let _ = this.update(cx, |pill, cx| {
                    if pill.pending_is(gen) {
                        pill.abandon_pending(true, cx);
                    }
                });
                return;
            };
            if !this.update(cx, |pill, _| pill.pending_is(gen)).unwrap_or(false) {
                return;
            }
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
                // Cancelada con el atajo mientras la ventana se estiraba.
                if !pill.pending_is(gen) {
                    return;
                }
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
        session.scroll = remembered_scroll();
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

    /// Captura desplazable: la ventana deja pasar el mouse (la rueda llega a
    /// la página), salvo sobre su barra.
    pub(crate) fn capture_passes_mouse(&self, cx: &App) -> bool {
        self.capture.as_ref().is_some_and(|view| view.read(cx).passes_mouse(self.cursor))
    }

    fn end_capture(&mut self, event: &CaptureEvent, window: &mut Window, cx: &mut Context<Self>) {
        // La mira sigue abierta, pero ya no tapa la pantalla: las otras
        // pantallas quedan libres.
        if let CaptureEvent::Scrolling = event {
            self.mirrors.close(cx);
            return;
        }
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
            // Al log de la pill (no solo a la salida, que se borra al reabrirla): si una
            // captura o un dibujo no llega al portapapeles, aquí queda por qué.
            Ok(saved) => {
                tracing::info!(
                    ancho = saved.width,
                    alto = saved.height,
                    archivo = %saved.path.display(),
                    ms = started.elapsed().as_millis() as u64,
                    "captura copiada y guardada"
                );
                // La cara hace «hecho» (en Expressive; en Atic no hace nada).
                self.look.pop_done(Instant::now());
                self.show_shelf(saved, from);
            }
            Err(error) => tracing::warn!(%error, "captura: no se pudo guardar ni copiar"),
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

    #[test]
    fn el_texto_se_lee_por_monitor() {
        // Como este Mac en puntos: la Retina (2×) a la izquierda y más baja,
        // el externo (1×) a la derecha; el cuadro va a 2×.
        let retina = PhysRect::new(-1470, 124, 1470, 956);
        let external = PhysRect::new(0, 0, 1920, 1080);
        let area = PhysRect::new(-1470, 0, 3390, 1080);
        let parts = monitor_parts(area, 2.0, (6780, 2160), &[external, retina]);
        assert_eq!(parts, vec![(2940, 0, 3840, 2160), (0, 248, 2940, 1912)]);
        // Un solo monitor (o la foto de uno): entera.
        assert!(monitor_parts(external, 1.0, (1920, 1080), &[external, retina]).is_empty());
        // En Windows, píxeles físicos (k = 1).
        let left = PhysRect::new(-1920, 0, 1920, 1080);
        let parts = monitor_parts(PhysRect::new(-1920, 0, 3840, 1080), 1.0, (3840, 1080), &[left, external]);
        assert_eq!(parts, vec![(0, 0, 1920, 1080), (1920, 0, 1920, 1080)]);
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
        let mut s = session(vec![Candidate { rect: Rect::new(10.0, 10.0, 20.0, 5.0), title: "Notas".into(), hwnd: 0 }]);
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
            hwnd: 0,
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
