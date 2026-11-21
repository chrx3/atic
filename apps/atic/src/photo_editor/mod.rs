//! El estilo de las capturas: el panel de la Pizarra que embellece la
//! captura mientras se anota. «Editar» en el estante (`shelf.rs`) abre la
//! Pizarra sobre la captura con este panel a la derecha
//! (`BoardView::start_polish` en `board.rs`): las herramientas de anotar
//! arriba, la captura con su fondo al centro y aquí los estilos rápidos, el
//! fondo (transparente, color, degradado o uno de los fondos del editor de
//! video), el margen, las esquinas, la sombra y la proporción. Copiar deja
//! captura + anotaciones + estilo en el portapapeles; Guardar escribe un PNG
//! nuevo «(editada)» junto a la original. La pestaña Texto lee el texto de la
//! captura y, en macOS, si se pide, lo traduce al lado
//! (`text_window::TextExtract`): el panel se ensancha para los dos lados y la
//! Pizarra reacomoda la foto.
//!
//! La composición es de CPU (`compose.rs`). El fondo de la vista previa se
//! arma solo al cambiar un ajuste o el recorte, con la captura achicada y en
//! otro hilo; lo que se copia o guarda se arma a tamaño completo, también en
//! otro hilo (`finish`). El último estilo queda en `photo-style.json` (la
//! carpeta de datos de la pill): la siguiente captura sale igual.

pub(crate) mod compose;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui::{
    div, img, linear_color_stop, linear_gradient, prelude::*, px, svg, AnyElement, App, ClickEvent, Context, CursorStyle,
    EventEmitter, Hsla, MouseButton, RenderImage, SharedString, Window,
};
use gpui_m3::theme::type_scale::{self, TypeScaleExt};
use gpui_m3::ButtonSize;
use image::RgbaImage;

use crate::i18n::t;
use crate::recording_editor::wallpapers;
use compose::{Background, Ratio, Style};

const GUTTER: f32 = 16.0;
/// El ancho del panel en la Pizarra.
pub const PANEL_W: f32 = 340.0;
/// El ancho en la pestaña Texto con la traducción abierta: el original y la traducción lado a lado.
pub const TEXT_PANEL_W: f32 = 640.0;
/// Lo que queda para los controles dentro del panel: sin el borde de 1 px de
/// cada lado ni los márgenes. (Contar el borde importa: con 2 px de más, tres
/// miniaturas no cabían y la grilla bajaba a dos columnas con un hueco.)
const INNER_W: f32 = PANEL_W - 2.0 - 2.0 * GUTTER;
/// El lado largo de la captura que se guarda para la vista previa.
const PREVIEW_SOURCE_MAX: u32 = 1600;
/// El lado largo del lienzo de la vista previa (con relleno y proporción).
const PREVIEW_MAX: u32 = 1600;
/// La separación entre miniaturas, igual a lo ancho y a lo alto.
const TILE_GAP: f32 = 6.0;
/// Colores y degradados: muestras cuadradas, dos filas de seis.
const COLOR_COLS: usize = 6;
/// Los fondos de imagen: cuatro por fila, en 16:10 (como una pantalla).
const PAPER_COLS: usize = 4;
const PAPER_RATIO: f32 = 10.0 / 16.0;
const PRESET_COLS: usize = 4;

/// Los colores sueltos: neutros y unos cuantos vivos.
const SOLIDS: [u32; 12] = [
    0xffffff, 0xf5f5f2, 0xe3e1dc, 0x9e9e9e, 0x3c3c3c, 0x1c1b1f, 0x1e88e5, 0x7e57c2, 0xec407a, 0xff7043, 0xffca28, 0x26a69a,
];

/// Los degradados además de los del editor de video.
const MORE_GRADIENTS: [(u32, u32); 6] =
    [(0xfc5c7d, 0x6a82fb), (0x00c6ff, 0x0072ff), (0xf7971e, 0xffd200), (0xa8edea, 0xfed6e3), (0x0f2027, 0x2c5364), (0x434343, 0x000000)];

fn style_file() -> Option<PathBuf> {
    crate::paths::file("photo-style.json")
}

/// El último estilo elegido; sin archivo, el de siempre.
fn load_style() -> Style {
    style_file()
        .and_then(|path| std::fs::read(path).ok())
        .and_then(|bytes| serde_json::from_slice::<Style>(&bytes).ok())
        .unwrap_or_default()
        .clamped()
}

fn save_style(style: &Style) {
    let Some(path) = style_file() else {
        return;
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(bytes) = serde_json::to_vec_pretty(style) {
        let _ = std::fs::write(path, bytes);
    }
}

/// Los degradados: los del editor de video y unos más.
fn gradients() -> Vec<(u32, u32)> {
    let mut out: Vec<(u32, u32)> = crate::edit_project::BACKDROPS
        .iter()
        .filter_map(|backdrop| match *backdrop {
            crate::edit_project::Backdrop::Gradient(a, b) => Some((a, b)),
            _ => None,
        })
        .collect();
    out.extend(MORE_GRADIENTS);
    out
}

/// Las pestañas del fondo.
#[derive(Clone, Copy, PartialEq)]
enum Kind {
    None,
    Color,
    Gradient,
    Image,
}

impl Kind {
    const ALL: [Kind; 4] = [Kind::None, Kind::Color, Kind::Gradient, Kind::Image];

    fn of(background: Background) -> Self {
        match background {
            Background::None => Self::None,
            Background::Solid(_) => Self::Color,
            Background::Gradient(..) => Self::Gradient,
            Background::Wallpaper(_) => Self::Image,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::None => "pill.photoEditor.bgNone",
            Self::Color => "pill.photoEditor.bgColor",
            Self::Gradient => "pill.photoEditor.bgGradient",
            Self::Image => "pill.photoEditor.bgImage",
        }
    }
}

/// El ancho de cada miniatura cuando `cols` llenan el panel (para su alto:
/// el ancho lo reparte la grilla).
fn tile_w(cols: usize) -> f32 {
    (INNER_W - TILE_GAP * (cols as f32 - 1.0)) / cols as f32
}

/// El alto de una miniatura de `cols` por fila cuya cara (sin el aro de 4 px
/// por lado) tiene proporción alto/ancho `ratio`.
fn tile_h(cols: usize, ratio: f32) -> f32 {
    const RING: f32 = 8.0;
    ((tile_w(cols) - RING) * ratio + RING).round()
}

fn hex(c: u32) -> Hsla {
    gpui::rgb(c).into()
}

fn rgb8(color: Hsla) -> [u8; 3] {
    let c = gpui::Rgba::from(color);
    [c.r, c.g, c.b].map(|v| (v * 255.0).round().clamp(0.0, 255.0) as u8)
}

/// De RGBA a lo que dibuja GPUI (BGRA).
fn render_image(rgba: RgbaImage) -> Arc<RenderImage> {
    let (w, h) = rgba.dimensions();
    let mut bgra = rgba.into_raw();
    for pixel in bgra.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    let buffer = RgbaImage::from_raw(w, h, bgra).unwrap_or_else(|| RgbaImage::new(1, 1));
    Arc::new(RenderImage::new([image::Frame::new(buffer)]))
}

/// De un cuadro de la Pizarra (BGRA con alfa directo) a RGBA.
fn rgba_of(frame: &atic_capture::Frame) -> Option<RgbaImage> {
    let mut rgba = frame.bgra.clone();
    for pixel in rgba.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    RgbaImage::from_raw(frame.width(), frame.height(), rgba)
}

fn encode_png(rgba: &RgbaImage) -> Result<Vec<u8>, String> {
    let mut png = Vec::new();
    rgba.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).map_err(|error| error.to_string())?;
    Ok(png)
}

/// Lo que se hace con la foto terminada.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Finish {
    Copy,
    Save,
}

/// Arma la foto final: `annotated` (la captura con lo dibujado y el recorte,
/// en BGRA) con `style`, y la copia o la guarda junto a `original`. Corre en
/// un hilo de fondo. Devuelve la ruta si la guardó.
pub fn finish(annotated: &atic_capture::Frame, style: &Style, finish: Finish, original: &Path, suffix: &str) -> Result<Option<PathBuf>, String> {
    let source = rgba_of(annotated).ok_or("imagen inválida")?;
    let out = compose::compose(&source, style);
    let png = encode_png(&out)?;
    match finish {
        Finish::Copy => {
            let (w, h) = out.dimensions();
            let mut bgra = out.into_raw();
            for pixel in bgra.chunks_exact_mut(4) {
                pixel.swap(0, 2);
            }
            crate::clip_image::write(w, h, &bgra, &png).map(|()| None)
        }
        Finish::Save => {
            let dir = original.parent().map(Path::to_path_buf).or_else(crate::paths::captures_dir).ok_or("sin carpeta de capturas")?;
            std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
            let target = compose::edited_path(original, &dir, suffix);
            std::fs::write(&target, &png).map_err(|error| error.to_string())?;
            Ok(Some(target))
        }
    }
}

/// Una parte de la captura, en píxeles: x, y, ancho y alto.
pub type Area = (u32, u32, u32, u32);

/// El fondo de la vista previa: la captura achicada con su estilo. La
/// Pizarra lo pone detrás de la foto, alrededor de `area`.
pub struct Preview {
    pub image: Arc<RenderImage>,
    pub style: Style,
    pub area: Area,
}

/// Lo que el panel le pide a la Pizarra.
pub enum StyleEvent {
    /// Cambió el estilo: el lienzo puede haber cambiado de tamaño.
    Changed,
    Finish(Finish),
}

/// Las pestañas del panel.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Mode {
    Style,
    Text,
}

/// Lo que se muestra sobre las acciones.
enum Note {
    Saved(PathBuf),
    Failed(String),
}

pub struct StylePanel {
    /// El tamaño de la captura entera, en píxeles.
    full: (u32, u32),
    /// La captura achicada para la vista previa, cuando ya está.
    small: Option<Arc<RgbaImage>>,
    /// Lo que sale (el recorte de la Pizarra); `None`, la captura entera.
    area: Option<Area>,
    style: Style,
    /// El último de cada tipo de fondo: volver a su pestaña lo trae de nuevo.
    last_solid: u32,
    last_gradient: (u32, u32),
    last_wallpaper: u8,
    preview: Option<Preview>,
    /// Los colores del damero de la vista previa (siguen al tema).
    checker: ([u8; 3], [u8; 3]),
    /// Sube con cada cambio de ajuste: una vista previa vieja que termina tarde se rehace.
    generation: u64,
    composing: bool,
    /// Copiando o guardando.
    busy: bool,
    note: Option<Note>,
    mode: Mode,
    /// La captura original, para leerle el texto.
    source: atic_capture::Frame,
    /// La pestaña Texto: se arma (y lee la captura) la primera vez que se abre.
    text: Option<gpui::Entity<crate::text_window::TextExtract>>,
    /// La pestaña Texto muestra la traducción al lado: el panel se ensancha.
    text_wide: bool,
    _text_events: Option<gpui::Subscription>,
}

impl EventEmitter<StyleEvent> for StylePanel {}

impl StylePanel {
    /// El panel para la captura `frame` (la foto de la Pizarra, en BGRA).
    pub fn new(frame: &atic_capture::Frame, cx: &mut Context<Self>) -> Self {
        let full = (frame.width(), frame.height());
        let source = frame.clone();
        let frame = frame.clone();
        // Achicar una pantalla entera toma su rato: va en otro hilo.
        cx.spawn(async move |this, cx| {
            let small = cx.background_spawn(async move { rgba_of(&frame).map(|image| Arc::new(compose::shrink(&image, PREVIEW_SOURCE_MAX))) }).await;
            let _ = this.update(cx, |panel, cx| {
                panel.small = small;
                panel.restyle(cx);
                cx.notify();
            });
        })
        .detach();
        let style = load_style();
        let gradient = match Style::default().background {
            Background::Gradient(a, b) => (a, b),
            _ => gradients()[0],
        };
        let mut panel = Self {
            full,
            small: None,
            area: None,
            style,
            last_solid: SOLIDS[0],
            last_gradient: gradient,
            last_wallpaper: wallpapers::ids().next().unwrap_or(0),
            preview: None,
            checker: ([255; 3], [230; 3]),
            generation: 0,
            composing: false,
            busy: false,
            note: None,
            mode: Mode::Style,
            source,
            text: None,
            text_wide: false,
            _text_events: None,
        };
        panel.remember(style.background);
        panel
    }

    /// Cambia de pestaña. Texto lee la captura la primera vez. Al abrir o
    /// cerrar la traducción el panel cambia de ancho y la Pizarra reacomoda la
    /// foto (`StyleEvent::Changed`).
    fn set_mode(&mut self, mode: Mode, cx: &mut Context<Self>) {
        if self.mode == mode {
            return;
        }
        self.mode = mode;
        if mode == Mode::Text && self.text.is_none() {
            let frame = self.source.clone();
            let text = cx.new(|cx| crate::text_window::TextExtract::new(move || Ok(frame), cx));
            self._text_events = Some(cx.subscribe(&text, |panel, text, _: &crate::text_window::TranslationToggled, cx| {
                panel.text_wide = text.read(cx).shows_translation();
                cx.emit(StyleEvent::Changed);
                cx.notify();
            }));
            self.text = Some(text);
        }
        cx.emit(StyleEvent::Changed);
        cx.notify();
    }

    /// El ancho que pide el panel: más ancho en la pestaña Texto con la traducción al lado.
    pub fn width(&self) -> f32 {
        if self.mode == Mode::Text && self.text_wide {
            TEXT_PANEL_W
        } else {
            PANEL_W
        }
    }

    /// Se está escribiendo en la pestaña Texto: las letras no son atajos de la Pizarra.
    pub fn has_text_focus(&self, window: &Window, cx: &App) -> bool {
        self.mode == Mode::Text && self.text.as_ref().is_some_and(|text| text.read(cx).has_focus(window, cx))
    }

    pub fn style(&self) -> Style {
        self.style
    }

    pub fn preview(&self) -> Option<&Preview> {
        self.preview.as_ref()
    }

    /// Lo que sale, en píxeles de la captura.
    fn area_or_full(&self) -> Area {
        self.area.unwrap_or((0, 0, self.full.0, self.full.1))
    }

    /// El recorte cambió: el fondo va alrededor de lo que sale.
    pub fn set_area(&mut self, area: Option<Area>, cx: &mut Context<Self>) {
        if self.area != area {
            self.area = area;
            self.restyle(cx);
            cx.notify();
        }
    }

    /// Empieza a copiar o guardar; `false` si ya está en eso.
    pub fn begin_finish(&mut self, cx: &mut Context<Self>) -> bool {
        if self.busy {
            return false;
        }
        self.busy = true;
        self.note = None;
        save_style(&self.style);
        cx.notify();
        true
    }

    /// Terminó de copiar o guardar (`Some(ruta)` si guardó).
    pub fn finished(&mut self, done: Result<Option<PathBuf>, String>, cx: &mut Context<Self>) {
        self.busy = false;
        match done {
            Ok(None) => {}
            Ok(Some(saved)) => self.note = Some(Note::Saved(saved)),
            Err(error) => {
                tracing::warn!(%error, "estilo de captura: no se pudo terminar");
                self.note = Some(Note::Failed(error));
            }
        }
        cx.notify();
    }

    fn remember(&mut self, background: Background) {
        match background {
            Background::Solid(c) => self.last_solid = c,
            Background::Gradient(a, b) => self.last_gradient = (a, b),
            Background::Wallpaper(id) => self.last_wallpaper = id,
            Background::None => {}
        }
    }

    /// Cambió un ajuste: se pide otra vista previa y la Pizarra reacomoda la foto.
    fn set_style(&mut self, change: impl FnOnce(&mut Style), cx: &mut Context<Self>) {
        let before = self.style;
        change(&mut self.style);
        self.style = self.style.clamped();
        if self.style != before {
            self.remember(self.style.background);
            self.note = None;
            self.restyle(cx);
            cx.emit(StyleEvent::Changed);
            cx.notify();
        }
    }

    /// Una pestaña del fondo: el último de ese tipo (o el primero).
    fn pick_kind(&mut self, kind: Kind, cx: &mut Context<Self>) {
        if Kind::of(self.style.background) == kind {
            return;
        }
        let background = match kind {
            Kind::None => Background::None,
            Kind::Color => Background::Solid(self.last_solid),
            Kind::Gradient => Background::Gradient(self.last_gradient.0, self.last_gradient.1),
            Kind::Image => Background::Wallpaper(self.last_wallpaper),
        };
        self.set_style(|style| style.background = background, cx);
    }

    /// Los estilos rápidos (conservan la proporción elegida).
    fn presets(&self) -> [(&'static str, Style); 4] {
        let ratio = self.style.ratio;
        let radius = compose::RADIUS_DEFAULT;
        let make = |background, padding, radius, shadow| Style { background, padding, radius, shadow, ratio };
        let (a, b) = self.last_gradient;
        [
            ("pill.photoEditor.presetOriginal", make(Background::None, 0.0, 0.0, 0.0)),
            ("pill.photoEditor.presetClean", make(Background::None, 0.05, radius, 0.6)),
            ("pill.photoEditor.bgGradient", make(Background::Gradient(a, b), 0.08, radius, 0.5)),
            ("pill.photoEditor.bgImage", make(Background::Wallpaper(self.last_wallpaper), 0.1, radius, 0.6)),
        ]
    }

    /// Rehace la vista previa. Si ya se está armando una, al terminar se arma
    /// otra con lo último (los deslizadores avisan a cada paso del arrastre).
    fn restyle(&mut self, cx: &mut Context<Self>) {
        self.generation += 1;
        if !self.composing {
            self.compose_preview(cx);
        }
    }

    fn compose_preview(&mut self, cx: &mut Context<Self>) {
        let Some(small) = self.small.clone() else {
            return;
        };
        let (style, generation, checker, area, full) = (self.style, self.generation, self.checker, self.area_or_full(), self.full);
        self.composing = true;
        cx.spawn(async move |this, cx| {
            let image = cx
                .background_spawn(async move {
                    // El recorte, llevado a la captura achicada.
                    let k = small.width() as f32 / full.0.max(1) as f32;
                    let piece = if area == (0, 0, full.0, full.1) {
                        None
                    } else {
                        let x = ((area.0 as f32 * k).round() as u32).min(small.width().saturating_sub(1));
                        let y = ((area.1 as f32 * k).round() as u32).min(small.height().saturating_sub(1));
                        let w = ((area.2 as f32 * k).round() as u32).clamp(1, small.width() - x);
                        let h = ((area.3 as f32 * k).round() as u32).clamp(1, small.height() - y);
                        Some(image::imageops::crop_imm(small.as_ref(), x, y, w, h).to_image())
                    };
                    let source = piece.as_ref().unwrap_or(small.as_ref());
                    // Con relleno y proporción el lienzo crece: se achica la captura para que quepa.
                    let canvas = compose::layout(source.dimensions(), &style).canvas;
                    let long = canvas.0.max(canvas.1);
                    let mut out = if long > PREVIEW_MAX {
                        let side = source.width().max(source.height()) as u64 * PREVIEW_MAX as u64 / long as u64;
                        compose::compose(&compose::shrink(source, side.max(1) as u32), &style)
                    } else {
                        compose::compose(source, &style)
                    };
                    // Sin fondo, el damero deja ver qué queda transparente.
                    if style.background == Background::None {
                        let cell = (out.width().max(out.height()) / 64).max(6);
                        compose::checker_under(&mut out, cell, checker.0, checker.1);
                    }
                    render_image(out)
                })
                .await;
            let _ = this.update(cx, |panel, cx| {
                panel.composing = false;
                let old = panel.preview.replace(Preview { image, style, area });
                crate::release::maybe(old.map(|preview| preview.image));
                if panel.generation != generation {
                    panel.compose_preview(cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    // --- La vista ------------------------------------------------------------------

    fn section_title(text: &'static str, s: &gpui_m3::Scheme) -> gpui::Div {
        div().text_color(s.on_surface).type_emphasized(type_scale::TITLE_SMALL).child(t(text))
    }

    fn section(children: Vec<AnyElement>) -> AnyElement {
        div().flex().flex_col().gap(px(12.)).children(children).into_any_element()
    }

    /// Una miniatura elegible, de alto `h` y a lo ancho de su celda: el aro de
    /// la elegida (o el de hover) y adentro `face`, con esquinas concéntricas
    /// (M afuera, S adentro: 12 = 8 + los 4 px del aro y su luz). La elegida
    /// lleva además una marca, que la distingue aunque el aro se pierda en un
    /// fondo del mismo color.
    fn tile(id: impl Into<gpui::ElementId>, h: f32, on: bool, face: gpui::Div, s: &gpui_m3::Scheme) -> gpui::Stateful<gpui::Div> {
        let hover = s.outline;
        let check = on.then(|| {
            div()
                .absolute()
                .top(px(5.))
                .right(px(5.))
                .size(px(16.))
                .rounded_full()
                .bg(s.primary)
                .border_1()
                .border_color(s.on_primary)
                .flex()
                .items_center()
                .justify_center()
                .child(svg().path("icons/check.svg").size(px(10.)).text_color(s.on_primary))
        });
        div()
            .id(id)
            .relative()
            .w_full()
            .h(px(h))
            .p(px(2.))
            .rounded(gpui_m3::corner::M)
            .border_2()
            .border_color(if on { s.primary } else { s.primary.opacity(0.0) })
            .when(!on, |el| el.hover(move |st| st.border_color(hover.opacity(0.6))))
            .cursor_pointer()
            .child(face.size_full().rounded(gpui_m3::corner::S).overflow_hidden())
            .children(check)
    }

    /// Filas de `cols` celdas iguales que llenan el ancho, con la misma
    /// separación a lo ancho y a lo alto. La última fila se completa con
    /// huecos para que sus celdas midan lo mismo que las de arriba.
    fn grid(cols: usize, children: impl IntoIterator<Item = AnyElement>) -> gpui::Div {
        let cell = |child: Option<AnyElement>| div().flex_1().min_w_0().children(child).into_any_element();
        let mut children = children.into_iter().peekable();
        let mut rows = Vec::new();
        while children.peek().is_some() {
            let row: Vec<AnyElement> = (0..cols).map(|_| cell(children.next())).collect();
            rows.push(div().w_full().flex().gap(px(TILE_GAP)).children(row));
        }
        div().w_full().flex().flex_col().gap(px(TILE_GAP)).children(rows)
    }

    fn presets_row(&self, s: &gpui_m3::Scheme, cx: &mut Context<Self>) -> AnyElement {
        let h = tile_h(PRESET_COLS, 0.72);
        let current = self.style;
        let items = self.presets().into_iter().enumerate().map(|(i, (label, preset))| {
            let on = current == preset;
            let face = match preset.background {
                Background::None => div().bg(s.surface_container_highest),
                Background::Solid(c) => div().bg(hex(c)),
                Background::Gradient(a, b) => div().bg(linear_gradient(135., linear_color_stop(hex(a), 0.), linear_color_stop(hex(b), 1.))),
                Background::Wallpaper(id) => div().child(
                    img(gpui::ImageSource::from(wallpapers::thumb_asset(id)))
                        .absolute()
                        .size_full()
                        .object_fit(gpui::ObjectFit::Cover),
                ),
            };
            // La captura en miniatura, con el margen, las esquinas y la sombra del estilo.
            let inset = (preset.padding * 70.0).round();
            let shot = div()
                .absolute()
                .top(px(inset))
                .bottom(px(inset))
                .left(px(inset))
                .right(px(inset))
                .bg(s.surface_container_lowest)
                .border_1()
                .border_color(s.outline_variant)
                .when(preset.radius > 0.0, |el| el.rounded(px(5.)))
                .when(preset.shadow > 0.0, |el| el.shadow_md());
            let tile = Self::tile(("photo-editor-preset", i), h, on, face.relative().child(shot), s)
                .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.set_style(|style| *style = preset, cx)));
            div()
                .w_full()
                .flex()
                .flex_col()
                .items_center()
                .gap(px(6.))
                .child(tile)
                .child(
                    div()
                        .w_full()
                        .text_center()
                        .overflow_hidden()
                        .text_ellipsis()
                        .whitespace_nowrap()
                        .text_color(if on { s.primary } else { s.on_surface_variant })
                        .type_style(type_scale::LABEL_MEDIUM)
                        .child(t(label)),
                )
                .into_any_element()
        });
        Self::grid(PRESET_COLS, items).into_any_element()
    }

    fn pick(background: Background, cx: &mut Context<Self>) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
        let view = cx.entity().downgrade();
        move |_, _, cx| {
            let _ = view.update(cx, |view, cx| view.set_style(|style| style.background = background, cx));
        }
    }

    fn background_body(&self, s: &gpui_m3::Scheme, cx: &mut Context<Self>) -> AnyElement {
        let current = self.style.background;
        match Kind::of(current) {
            Kind::None => div()
                .px(px(12.))
                .py(px(10.))
                .rounded(gpui_m3::corner::M)
                .bg(s.surface_container)
                .text_color(s.on_surface_variant)
                .type_style(type_scale::BODY_SMALL)
                .child(t("pill.photoEditor.noneHint"))
                .into_any_element(),
            Kind::Color => {
                let h = tile_h(COLOR_COLS, 1.0);
                let items = SOLIDS.iter().enumerate().map(|(i, &c)| {
                    let background = Background::Solid(c);
                    let face = div().bg(hex(c)).border_1().border_color(s.outline_variant);
                    Self::tile(("photo-editor-solid", i), h, current == background, face, s).on_click(Self::pick(background, cx)).into_any_element()
                });
                Self::grid(COLOR_COLS, items).into_any_element()
            }
            Kind::Gradient => {
                let h = tile_h(COLOR_COLS, 1.0);
                let items = gradients().into_iter().enumerate().map(|(i, (a, b))| {
                    let background = Background::Gradient(a, b);
                    let face = div().bg(linear_gradient(135., linear_color_stop(hex(a), 0.), linear_color_stop(hex(b), 1.)));
                    Self::tile(("photo-editor-gradient", i), h, current == background, face, s).on_click(Self::pick(background, cx)).into_any_element()
                });
                Self::grid(COLOR_COLS, items).into_any_element()
            }
            Kind::Image => {
                let h = tile_h(PAPER_COLS, PAPER_RATIO);
                let placeholder_bg = s.surface_container_high;
                let placeholder = move || div().size_full().bg(placeholder_bg).into_any_element();
                let items = wallpapers::ids().map(|id| {
                    let background = Background::Wallpaper(id);
                    let face = div().child(
                        img(gpui::ImageSource::from(wallpapers::thumb_asset(id)))
                            .size_full()
                            .object_fit(gpui::ObjectFit::Cover)
                            .with_loading(placeholder)
                            .with_fallback(placeholder),
                    );
                    Self::tile(("photo-editor-paper", id as usize), h, current == background, face, s)
                        .on_click(Self::pick(background, cx))
                        .into_any_element()
                });
                // Las miniaturas, en su propia caché: se sueltan al cerrar la Pizarra.
                gpui::image_cache(gpui::retain_all("photo-editor-papers")).w_full().child(Self::grid(PAPER_COLS, items)).into_any_element()
            }
        }
    }

    fn slider_row(
        &self,
        id: &'static str,
        label: &'static str,
        value: f32,
        max: f32,
        s: &gpui_m3::Scheme,
        set: fn(&mut Style, f32),
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let view = cx.entity().downgrade();
        let percent = move |value: f32| format!("{}", (value / max * 100.0).round() as i32);
        div()
            .flex()
            .flex_col()
            .gap(px(2.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(div().text_color(s.on_surface_variant).type_style(type_scale::LABEL_LARGE).child(t(label)))
                    .child(div().text_color(s.on_surface).type_emphasized(type_scale::LABEL_LARGE).child(percent(value))),
            )
            .child(
                gpui_m3::Slider::new(id, value, 0.0..=max)
                    .width(px(INNER_W))
                    .value_indicator(true)
                    .format(percent)
                    .on_change(move |value, _, cx| {
                        let _ = view.update(cx, |view, cx| view.set_style(|style| set(style, value), cx));
                    }),
            )
            .into_any_element()
    }

    fn controls(&self, s: &gpui_m3::Scheme, cx: &mut Context<Self>) -> AnyElement {
        let style = self.style;
        let view = cx.entity().downgrade();
        let ratio = Ratio::ALL.iter().position(|&r| r == style.ratio).unwrap_or(0);
        let ratios = Ratio::ALL.iter().map(|r| {
            gpui_m3::Segment::label(match r.dims() {
                None => SharedString::from(t("pill.photoEditor.ratioAuto")),
                Some((w, h)) => SharedString::from(format!("{w}:{h}")),
            })
        });
        let ratio_picker = gpui_m3::SegmentedButtons::new("photo-editor-ratio", ratios.collect::<Vec<_>>(), ratio).dense(true).sliding(true).on_change({
            let view = view.clone();
            move |index, _, cx| {
                let ratio = Ratio::ALL.get(index).copied().unwrap_or_default();
                let _ = view.update(cx, |view, cx| view.set_style(|style| style.ratio = ratio, cx));
            }
        });
        let kind = Kind::ALL.iter().position(|&k| k == Kind::of(style.background)).unwrap_or(0);
        let kinds = Kind::ALL.iter().map(|k| gpui_m3::Segment::label(t(k.label()))).collect::<Vec<_>>();
        let kind_picker = gpui_m3::SegmentedButtons::new("photo-editor-kind", kinds, kind).dense(true).sliding(true).on_change(move |index, _, cx| {
            let kind = Kind::ALL.get(index).copied().unwrap_or(Kind::None);
            let _ = view.update(cx, |view, cx| view.pick_kind(kind, cx));
        });
        let defaults = Style::default();
        let frame_is_default = style.padding == defaults.padding && style.radius == defaults.radius && style.shadow == defaults.shadow;
        let reset = gpui_m3::Button::new("photo-editor-reset", t("pill.photoEditor.reset"))
            .text()
            .size(ButtonSize::Tiny)
            .icon("icons/rotate-ccw.svg")
            .disabled(frame_is_default)
            .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                view.set_style(
                    |style| {
                        style.padding = defaults.padding;
                        style.radius = defaults.radius;
                        style.shadow = defaults.shadow;
                    },
                    cx,
                )
            }));
        div()
            .id("photo-editor-side-scroll")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px(px(GUTTER))
            .pt(px(4.))
            .pb(px(24.))
            .flex()
            .flex_col()
            .gap(px(24.))
            .child(Self::section(vec![Self::section_title("pill.photoEditor.quickStyles", s).into_any_element(), self.presets_row(s, cx)]))
            .child(div().h(px(1.)).flex_none().bg(s.outline_variant))
            .child(Self::section(vec![
                Self::section_title("pill.editor.background", s).into_any_element(),
                div().flex().child(kind_picker).into_any_element(),
                self.background_body(s, cx),
            ]))
            .child(Self::section(vec![
                div().h(px(28.)).flex().items_center().justify_between().child(Self::section_title("pill.photoEditor.frame", s)).child(reset).into_any_element(),
                self.slider_row("photo-editor-padding", "pill.editor.padding", style.padding, compose::PADDING_MAX, s, |st, v| st.padding = v, cx),
                self.slider_row("photo-editor-radius", "pill.editor.corners", style.radius, compose::RADIUS_MAX, s, |st, v| st.radius = v, cx),
                self.slider_row("photo-editor-shadow", "pill.editor.shadow", style.shadow, 1.0, s, |st, v| st.shadow = v, cx),
            ]))
            .child(Self::section(vec![
                Self::section_title("pill.photoEditor.ratio", s).into_any_element(),
                div().flex().child(ratio_picker).into_any_element(),
            ]))
            .into_any_element()
    }

    /// Copiar (la principal) y Guardar, siempre a la vista al pie de la pestaña Estilo.
    fn actions(&self, s: &gpui_m3::Scheme, cx: &mut Context<Self>) -> AnyElement {
        let ready = !self.busy;
        let note = self.note.as_ref().map(|note| match note {
            Note::Saved(path) => {
                let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                let reveal = path.clone();
                div()
                    .id("photo-editor-saved")
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .px(px(12.))
                    .py(px(8.))
                    .rounded(gpui_m3::corner::M)
                    .bg(s.secondary_container)
                    .text_color(s.on_secondary_container)
                    .cursor_pointer()
                    .tooltip(crate::hover::tip(t("pill.photoEditor.showFile")))
                    .on_click(move |_, _, _| {
                        let _ = crate::paths::reveal(&reveal);
                    })
                    .child(svg().path("icons/check.svg").size(px(14.)).flex_none().text_color(s.on_secondary_container))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .type_style(type_scale::BODY_SMALL)
                            .child(crate::i18n::tf("pill.photoEditor.saved", &[("name", &name)])),
                    )
                    .child(svg().path("icons/folder.svg").size(px(14.)).flex_none().text_color(s.on_secondary_container))
                    .into_any_element()
            }
            Note::Failed(error) => div()
                .px(px(12.))
                .py(px(8.))
                .rounded(gpui_m3::corner::M)
                .bg(s.error_container)
                .text_color(s.on_error_container)
                .type_style(type_scale::BODY_SMALL)
                .child(crate::i18n::tf("pill.photoEditor.failed", &[("error", error)]))
                .into_any_element(),
        });
        // Guardar y Copiar (la principal) a la derecha, como las acciones de un diálogo.
        div()
            .flex_none()
            .px(px(GUTTER))
            .pt(px(12.))
            .pb(px(16.))
            .flex()
            .flex_col()
            .gap(px(10.))
            .border_t_1()
            .border_color(s.outline_variant)
            .children(note)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_end()
                    .gap(px(8.))
                    .child(
                        gpui_m3::Button::new("photo-editor-save", t("pill.photoEditor.save"))
                            .tonal()
                            .size(ButtonSize::Comfortable)
                            .icon("icons/download.svg")
                            .tooltip(crate::keymap::hint(t("pill.photoEditor.saveTip")))
                            .disabled(!ready)
                            .on_click(cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(StyleEvent::Finish(Finish::Save)))),
                    )
                    .child(
                        gpui_m3::Button::new("photo-editor-copy", t("shelf.copy"))
                            .filled()
                            .size(ButtonSize::Comfortable)
                            .icon("icons/copy.svg")
                            .tooltip(crate::keymap::hint(t("pill.photoEditor.copyTip")))
                            .disabled(!ready)
                            .on_click(cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(StyleEvent::Finish(Finish::Copy)))),
                    ),
            )
            .into_any_element()
    }
}

impl Render for StylePanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Va dentro de la Pizarra, que ya fijó el tema de M3 (`scope_m3`).
        let s = gpui_m3::Theme::of(cx).clone();
        // El damero sigue al tema: si cambió, la vista previa sin fondo se rehace (una vez).
        let checker = (rgb8(s.surface_container_lowest), rgb8(s.surface_container_highest));
        if checker != self.checker {
            self.checker = checker;
            if self.style.background == Background::None {
                self.restyle(cx);
            }
        }
        // Las pestañas, y en Estilo el tamaño de lo que sale al copiar o guardar.
        let tabs = gpui_m3::SegmentedButtons::new(
            "photo-editor-mode",
            vec![
                gpui_m3::Segment::label(t("pill.photoEditor.styleTitle")).with_icon("icons/brush.svg"),
                gpui_m3::Segment::label(t("pill.photoEditor.textTab")).with_icon("icons/scan-text.svg"),
            ],
            if self.mode == Mode::Style { 0 } else { 1 },
        )
        .dense(true)
        .sliding(true)
        .on_change({
            let view = cx.entity().downgrade();
            move |index, _, cx| {
                let mode = if index == 0 { Mode::Style } else { Mode::Text };
                let _ = view.update(cx, |panel, cx| panel.set_mode(mode, cx));
            }
        });
        let size = (self.mode == Mode::Style).then(|| {
            let area = self.area_or_full();
            let (w, h) = compose::layout((area.2, area.3), &self.style).canvas;
            div()
                .flex_none()
                .px(px(10.))
                .py(px(4.))
                .rounded_full()
                .bg(s.surface_container_high)
                .text_color(s.on_surface_variant)
                .type_style(type_scale::LABEL_MEDIUM)
                .child(format!("{w} × {h} px"))
        });
        let header = div()
            .flex_none()
            .px(px(GUTTER))
            .pt(px(14.))
            .pb(px(12.))
            .flex()
            .items_center()
            .justify_between()
            .gap(px(8.))
            // Las anclas de la guía del editor (`board.rs`, `PHOTO_GUIDE`).
            .child(crate::coach::anchor("photo.text", div().flex_none().child(tabs)))
            .children(size);
        let panel = div()
            .id("photo-style-panel")
            .size_full()
            .flex()
            .flex_col()
            .overflow_hidden()
            .rounded(gpui_m3::corner::XL)
            .bg(s.surface_container_low)
            .border_1()
            .border_color(s.outline_variant)
            .shadow_lg()
            .font_family(crate::theme::font())
            .text_color(s.on_surface)
            .cursor(CursorStyle::Arrow)
            // Lo que pasa en el panel no dibuja en la Pizarra ni la mueve.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
            .child(header);
        match (self.mode, self.text.clone()) {
            (Mode::Text, Some(text)) => panel.child(div().flex_1().min_h_0().flex().flex_col().px(px(GUTTER - 4.0)).pb(px(GUTTER - 4.0)).child(text)),
            _ => {
                let controls = self.controls(&s, cx);
                let actions = self.actions(&s, cx);
                panel.child(crate::coach::anchor("photo.style", controls)).child(crate::coach::anchor("photo.finish", actions))
            }
        }
    }
}

impl Drop for StylePanel {
    fn drop(&mut self) {
        save_style(&self.style);
        crate::release::maybe(self.preview.take().map(|preview| preview.image));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guardar_deja_la_anotada_con_su_estilo_junto_a_la_original() {
        let dir = std::env::temp_dir().join(format!("atic-photo-style-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let original = dir.join("Captura 2.png");
        // Lo anotado llega en BGRA: un rojo puro.
        let bounds = atic_capture::Rect { x: 0, y: 0, width: 40, height: 30 };
        let annotated = atic_capture::Frame::new(bounds, [0u8, 0, 255, 255].repeat(40 * 30));
        let style = Style { background: Background::Solid(0x336699), padding: 0.2, radius: 0.0, shadow: 0.0, ratio: Ratio::Auto };
        let saved = finish(&annotated, &style, Finish::Save, &original, "(editada)").unwrap().unwrap();
        assert_eq!(saved, dir.join("Captura 2 (editada).png"));
        let out = image::open(&saved).unwrap().to_rgba8();
        assert_eq!(out.dimensions(), (52, 42), "el margen va alrededor");
        assert_eq!(out.get_pixel(0, 0).0, [0x33, 0x66, 0x99, 255], "el fondo del estilo");
        assert_eq!(out.get_pixel(26, 21).0, [255, 0, 0, 255], "la captura, en RGBA");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
