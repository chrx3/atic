//! Apariencia de la pill: cuánto se ve el vidrio y cuánto se tapa para leer.
//!
//! El tinte de la pill sobre el vidrio es un compromiso: con poco se ve el
//! desenfoque, con mucho se lee bien el texto sobre fondos claros. Se elige en
//! la sección Apariencia de los Ajustes (`settings.rs`), con la pill a la
//! vista: los cambios se aplican en el cuadro siguiente y se guardan al soltar
//! el deslizador (`<datos de Atic>\pill\appearance.json`).
//!
//! Ahí mismo se elige el estilo de las ventanas (Ajustes, Reuniones y el
//! espacio): el de Atic de siempre, Material 3 Expressive o Liquid Glass, como
//! los estilos de Atic Code (`theme::styled`).

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Mutex;

use gpui::{
    canvas, div, prelude::*, px, Bounds, ClickEvent, Context, MouseButton, MouseMoveEvent,
    MouseUpEvent, Pixels, SharedString, Window,
};
use serde::{Deserialize, Serialize};

use crate::hover::HoverExt;
use crate::settings::{card, hsla, row, switch_row, text_button, AMBER, FILL, MUTED, TEXT, TRACK};
use gpui_m3::theme::type_scale::{self, TypeScaleExt};

#[derive(Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Appearance {
    /// El escritorio desenfocado detrás de la pill (`glass.rs`).
    pub glass: bool,
    /// Opacidad del tinte con la pill en reposo (0..1).
    pub rest_tint: f32,
    /// Opacidad del tinte con algo para leer: paneles y vistazos.
    pub read_tint: f32,
    /// La letra de la canción y los contadores del tab también se tapan como
    /// un panel. Apagado, se quedan sobre el vidrio con el tinte de reposo.
    pub cover_hang: bool,
    /// El estilo de las ventanas.
    pub windows: WindowStyle,
    /// Lo que tapan los fondos de las ventanas en Liquid Glass (0..1; 0,5 como Atic Code).
    pub glass_opacity: f32,
    /// El estilo y la opacidad ya son los mismos que los de Atic Code: la
    /// primera vez se toman de allá (`load`).
    pub linked: bool,
    /// El color de la piel de la pill y del notch.
    pub skin: Skin,
    /// El estilo de la pill (`pill_look.rs`).
    pub pill: PillStyle,
    /// Lo último que se armó en «Color propio» y «Degradado»: se guarda
    /// aunque después elijas un color de la lista.
    pub custom: CustomColors,
    /// Expressive: la semilla del esquema, si no es la piel.
    pub accent: Option<u32>,
    /// Expressive: el estilo de color del esquema (la variante de M3).
    pub scheme: SchemeStyle,
    /// Expressive: el contraste del esquema, de -1 a 1 (0 es el estándar).
    pub contrast: f32,
    /// Expressive: la piel pinta la pill (lisa o en degradado). Apagado, la
    /// pill va en el contenedor del esquema y la piel solo lo tiñe.
    pub skin_fill: bool,
    /// Expressive: el tema de la pill y de sus paneles (en Atic, siempre oscura).
    pub pill_theme: PillTheme,
}

/// Expressive: el tema del esquema de la pill. Por defecto, el mismo de las
/// ventanas («Tema de las ventanas»): así la pill y Ajustes, Reuniones y Atic
/// Code cambian juntos. Se guarda como `system` por compatibilidad.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PillTheme {
    Dark,
    Light,
    #[default]
    System,
}

impl PillTheme {
    /// En el orden de Apariencia, con la clave `i18n` de su nombre.
    pub const ALL: [(PillTheme, &'static str); 3] = [
        (PillTheme::System, "pill.appearance.pillThemeSystem"),
        (PillTheme::Light, "pill.appearance.pillThemeLight"),
        (PillTheme::Dark, "pill.appearance.pillThemeDark"),
    ];

    /// ¿Va oscura? `windows_light`: si las ventanas están en claro (solo
    /// cuenta con «Igual que las ventanas»).
    pub fn dark(self, windows_light: bool) -> bool {
        match self {
            PillTheme::Dark => true,
            PillTheme::Light => false,
            PillTheme::System => !windows_light,
        }
    }
}

/// Cómo se pinta la pill: la de siempre, o Material 3 Expressive como toolbar
/// flotante (capa de estado, píldora de la elegida, forma de la cara).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PillStyle {
    #[default]
    Atic,
    Expressive,
}

impl PillStyle {
    /// El nombre (de marca, no se traduce) y la clave `i18n` de su ayuda.
    const ALL: [(PillStyle, &'static str, &'static str); 2] = [
        (PillStyle::Atic, "Atic", "pill.appearance.pillStyleAticHint"),
        (PillStyle::Expressive, "M3 Expressive", "pill.appearance.pillStyleExpressiveHint"),
    ];
}

/// El color de la piel: el de Atic, uno liso o un degradado de dos o tres
/// colores. Si el texto y los íconos encima no se leen (una piel clara con
/// el tema oscuro, o una oscura con el claro de Expressive), se tapa con un
/// velo (`PillTokens::veil`) hasta que se lean.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Skin {
    #[default]
    Atic,
    Solid { color: u32 },
    /// De `from` a `to`, con `via` al medio si lo hay, hacia `angle` (grados
    /// como en CSS: 0 hacia arriba, 90 de izquierda a derecha).
    Gradient {
        from: u32,
        to: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        via: Option<u32>,
        #[serde(default = "gradient_angle")]
        angle: u16,
    },
}

/// La dirección de las pieles degradadas de antes de poder elegirla.
const GRADIENT_ANGLE: u16 = 90;

fn gradient_angle() -> u16 {
    GRADIENT_ANGLE
}

/// Un degradado de la lista: dos colores, de izquierda a derecha.
const fn duo(from: u32, to: u32) -> Skin {
    Skin::Gradient { from, to, via: None, angle: GRADIENT_ANGLE }
}

/// Los que se ofrecen en Apariencia, con su nombre.
const SKINS: [(Skin, &str); 13] = [
    (Skin::Atic, "pill.appearance.skinAtic"),
    (Skin::Solid { color: 0x000000 }, "pill.appearance.skinBlack"),
    (Skin::Solid { color: 0x26272c }, "pill.appearance.skinGraphite"),
    (Skin::Solid { color: 0x14213d }, "pill.appearance.skinNight"),
    (Skin::Solid { color: 0x123524 }, "pill.appearance.skinForest"),
    (Skin::Solid { color: 0x3b1220 }, "pill.appearance.skinWine"),
    (Skin::Solid { color: 0x2a1640 }, "pill.appearance.skinPlum"),
    (duo(0x0f0c29, 0x302b63), "pill.appearance.skinMidnight"),
    (duo(0x0f2027, 0x2c5364), "pill.appearance.skinOcean"),
    (duo(0x093028, 0x237a57), "pill.appearance.skinAurora"),
    (duo(0x2b1055, 0x7d2a5c), "pill.appearance.skinDusk"),
    (duo(0x1f0a0a, 0x6a2614), "pill.appearance.skinEmber"),
    (duo(0x0b0b0c, 0x3a3b40), "pill.appearance.skinSteel"),
];

impl Skin {
    /// Las dos puntas (iguales si es liso).
    pub fn ends(self) -> (u32, u32) {
        match self {
            Skin::Atic => (SKIN, SKIN),
            Skin::Solid { color } => (color, color),
            Skin::Gradient { from, to, .. } => (from, to),
        }
    }

    /// Sus colores en orden: uno si es lisa, dos o tres si es degradada.
    pub fn stops(self) -> Vec<gpui::Hsla> {
        let hexes = match self {
            Skin::Gradient { from, to, via: Some(via), .. } => vec![from, via, to],
            _ => {
                let (a, b) = self.ends();
                if a == b { vec![a] } else { vec![a, b] }
            }
        };
        hexes.into_iter().map(|hex| gpui::rgb(hex).into()).collect()
    }

    /// El color que la representa donde no cabe un degradado (aros, fundidos
    /// sobre la piel): el del medio.
    pub fn mid(self) -> gpui::Hsla {
        if let Skin::Gradient { via: Some(via), .. } = self {
            return gpui::rgb(via).into();
        }
        let (a, b) = self.ends();
        let mix = |shift: u32| (((a >> shift) & 0xff) + ((b >> shift) & 0xff)) / 2;
        gpui::rgb((mix(16) << 16) | (mix(8) << 8) | mix(0)).into()
    }

    /// Está en la lista de Apariencia (si no, es un color o un degradado propio).
    pub fn is_preset(self) -> bool {
        SKINS.iter().any(|(skin, _)| *skin == self)
    }

    fn angle(self) -> f32 {
        match self {
            Skin::Gradient { angle, .. } => (angle % 360) as f32,
            _ => GRADIENT_ANGLE as f32,
        }
    }

    /// Para pintar en un solo fondo: liso o el degradado de punta a punta (con
    /// tres colores, sin el del medio; `layers` lo pinta entero).
    pub fn background(self, opacity: f32) -> gpui::Background {
        let (a, b) = self.ends();
        let (a, b): (gpui::Hsla, gpui::Hsla) = (gpui::rgb(a).into(), gpui::rgb(b).into());
        if a == b {
            return a.opacity(opacity).into();
        }
        gpui::linear_gradient(
            self.angle(),
            gpui::linear_color_stop(a.opacity(opacity), 0.),
            gpui::linear_color_stop(b.opacity(opacity), 1.),
        )
    }

    /// Los fondos que hay que pintar uno sobre otro: GPUI hace degradados de
    /// dos colores, así que uno de tres es la primera mitad (`from` → `via`,
    /// que sigue en `via` hasta el final) y encima la segunda (`to`, que
    /// aparece desde la mitad). Translúcida, la segunda capa taparía más que
    /// la primera: ahí va de punta a punta, sin el del medio.
    pub fn layers(self, opacity: f32) -> (gpui::Background, Option<gpui::Background>) {
        let Skin::Gradient { from, to, via: Some(via), .. } = self else {
            return (self.background(opacity), None);
        };
        if opacity < 0.999 {
            return (self.background(opacity), None);
        }
        let color = |hex: u32| -> gpui::Hsla { gpui::rgb(hex).into() };
        let angle = self.angle();
        (
            gpui::linear_gradient(angle, gpui::linear_color_stop(color(from), 0.), gpui::linear_color_stop(color(via), 0.5)),
            Some(gpui::linear_gradient(
                angle,
                gpui::linear_color_stop(color(to).opacity(0.), 0.5),
                gpui::linear_color_stop(color(to), 1.),
            )),
        )
    }

    /// Pinta la piel en la silueta `path`, con el velo encima si hace falta.
    pub fn paint(self, window: &mut Window, path: gpui::Path<Pixels>, opacity: f32, veil: Option<gpui::Hsla>) {
        let (base, over) = self.layers(opacity);
        if over.is_none() && veil.is_none() {
            window.paint_path(path, base);
            return;
        }
        window.paint_path(path.clone(), base);
        if let Some(over) = over {
            window.paint_path(path.clone(), over);
        }
        if let Some(veil) = veil {
            window.paint_path(path, veil.opacity(opacity));
        }
    }

    /// Lo mismo que `paint`, como fondo de `el`: las capas de encima van como
    /// hijos absolutos con la forma que les da `shape` (las esquinas de `el`).
    /// Llámala antes de agregarle hijos a `el`, para que queden debajo.
    pub fn fill_div(
        self,
        el: gpui::Div,
        opacity: f32,
        veil: Option<gpui::Hsla>,
        shape: impl Fn(gpui::Div) -> gpui::Div,
    ) -> gpui::Div {
        let (base, over) = self.layers(opacity);
        let layer = |background: gpui::Background| shape(div().absolute().inset_0().bg(background));
        el.relative()
            .bg(base)
            .when_some(over, |el, over| el.child(layer(over)))
            .when_some(veil, |el, veil| el.child(layer(veil.opacity(opacity).into())))
    }
}

/// «Color propio» y «Degradado» tal como se dejaron.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CustomColors {
    pub color: u32,
    pub from: u32,
    pub via: Option<u32>,
    pub to: u32,
    pub angle: u16,
}

impl Default for CustomColors {
    fn default() -> Self {
        Self { color: 0x1f3b5c, from: 0x2b1055, via: None, to: 0x0e5e6f, angle: 135 }
    }
}

impl CustomColors {
    pub fn solid(self) -> Skin {
        Skin::Solid { color: self.color }
    }

    pub fn gradient(self) -> Skin {
        Skin::Gradient { from: self.from, to: self.to, via: self.via, angle: self.angle % 360 }
    }
}

/// Expressive: cómo reparte el color el esquema (las variantes de M3).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SchemeStyle {
    #[default]
    TonalSpot,
    Expressive,
    Vibrant,
    Neutral,
    Monochrome,
    Fidelity,
    Content,
    Rainbow,
    FruitSalad,
}

impl SchemeStyle {
    /// Todas, con la clave `i18n` de su nombre.
    pub const ALL: [(SchemeStyle, &'static str); 9] = [
        (SchemeStyle::TonalSpot, "pill.appearance.schemeTonalSpot"),
        (SchemeStyle::Expressive, "pill.appearance.schemeExpressive"),
        (SchemeStyle::Vibrant, "pill.appearance.schemeVibrant"),
        (SchemeStyle::Neutral, "pill.appearance.schemeNeutral"),
        (SchemeStyle::Monochrome, "pill.appearance.schemeMonochrome"),
        (SchemeStyle::Fidelity, "pill.appearance.schemeFidelity"),
        (SchemeStyle::Content, "pill.appearance.schemeContent"),
        (SchemeStyle::Rainbow, "pill.appearance.schemeRainbow"),
        (SchemeStyle::FruitSalad, "pill.appearance.schemeFruitSalad"),
    ];

    pub fn variant(self) -> gpui_m3::SchemeVariant {
        use gpui_m3::SchemeVariant as V;
        match self {
            SchemeStyle::TonalSpot => V::TonalSpot,
            SchemeStyle::Expressive => V::Expressive,
            SchemeStyle::Vibrant => V::Vibrant,
            SchemeStyle::Neutral => V::Neutral,
            SchemeStyle::Monochrome => V::Monochrome,
            SchemeStyle::Fidelity => V::Fidelity,
            SchemeStyle::Content => V::Content,
            SchemeStyle::Rainbow => V::Rainbow,
            SchemeStyle::FruitSalad => V::FruitSalad,
        }
    }
}

/// Los contrastes que se ofrecen: estándar, medio y alto (como en Android).
const CONTRASTS: [(f32, &str); 3] = [
    (0.0, "pill.appearance.contrastStandard"),
    (0.5, "pill.appearance.contrastMedium"),
    (1.0, "pill.appearance.contrastHigh"),
];

/// Material 3 Expressive es el único estilo de las ventanas: «Atic» (Formal en Atic Code) y
/// Liquid Glass siguen en el código, pero no se ofrecen ni se aplican. Para volver a
/// mostrarlos, ponla en `false`.
pub const ONLY_EXPRESSIVE: bool = true;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WindowStyle {
    /// La paleta de siempre de Atic.
    #[default]
    Atic,
    Expressive,
    Glass,
}

impl WindowStyle {
    /// El nombre (de marca, no se traduce) y la clave `i18n` de su ayuda.
    const ALL: [(WindowStyle, &'static str, &'static str); 3] = [
        (WindowStyle::Atic, "Atic", "pill.appearance.aticHint"),
        (WindowStyle::Expressive, "Material 3 Expressive", "pill.appearance.expressiveHint"),
        (WindowStyle::Glass, "Liquid Glass", "pill.appearance.glassHint"),
    ];

    /// El de las ventanas que va con un estilo de Atic Code.
    pub fn of_code(code: crate::code::style::Style) -> Self {
        match code {
            crate::code::style::Style::Formal => WindowStyle::Atic,
            crate::code::style::Style::Expressive => WindowStyle::Expressive,
            crate::code::style::Style::Glass => WindowStyle::Glass,
        }
    }

    /// El estilo de Atic Code que va con este: es una sola elección para
    /// todas las ventanas («Atic» es «Formal» en Atic Code).
    pub fn linked_code_style(self) -> crate::code::style::Style {
        self.code_style().unwrap_or(crate::code::style::Style::Formal)
    }

    /// El estilo de Atic Code que le presta sus tokens; `None` con «Atic».
    pub fn code_style(self) -> Option<crate::code::style::Style> {
        match self {
            WindowStyle::Atic => None,
            WindowStyle::Expressive => Some(crate::code::style::Style::Expressive),
            WindowStyle::Glass => Some(crate::code::style::Style::Glass),
        }
    }
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            // Opaca por omisión: el vidrio y la transparencia se encienden en
            // Apariencia.
            glass: false,
            rest_tint: 1.0,
            read_tint: 1.0,
            cover_hang: true,
            windows: WindowStyle::Atic,
            glass_opacity: 0.5,
            linked: false,
            skin: Skin::Atic,
            pill: PillStyle::Atic,
            custom: CustomColors::default(),
            accent: None,
            scheme: SchemeStyle::TonalSpot,
            contrast: 0.0,
            skin_fill: true,
            pill_theme: PillTheme::System,
        }
    }
}

const REST_RANGE: (f32, f32) = (0.15, 1.0);
const READ_RANGE: (f32, f32) = (0.5, 1.0);

static CURRENT: Mutex<Option<Appearance>> = Mutex::new(None);

/// La pill no se redibuja sola en reposo: al arrastrar un color o el ángulo se
/// le avisa solo a ella. `refresh_windows` redibujaba todas las ventanas sin
/// caché (Atic Code incluido) en cada movimiento del mouse.
fn notify_pill(cx: &mut gpui::App) {
    if let Some(pill) = cx.windows().into_iter().find_map(|window| window.downcast::<crate::Pill>()) {
        let _ = pill.update(cx, |_, _, cx| cx.notify());
    }
}

/// La apariencia vigente. La pill la lee en cada cuadro.
pub fn current() -> Appearance {
    match CURRENT.lock() {
        Ok(mut current) => *current.get_or_insert_with(load),
        Err(_) => Appearance::default(),
    }
}

fn apply(mut look: Appearance, persist: bool) {
    if ONLY_EXPRESSIVE {
        look.windows = WindowStyle::Expressive;
    }
    if let Ok(mut current) = CURRENT.lock() {
        *current = Some(look);
    }
    if persist {
        save(&look);
    }
}

/// Pasa la piel de la pill a la siguiente de la lista y la guarda (la ficha
/// «Color» del vistazo de la cara). Una piel que no está en la lista vuelve a
/// la primera.
pub fn next_skin() -> Skin {
    let mut look = current();
    let next = SKINS.iter().position(|(skin, _)| *skin == look.skin).map_or(0, |i| (i + 1) % SKINS.len());
    look.skin = SKINS[next].0;
    apply(look, true);
    look.skin
}

/// El nombre de una piel de la lista, o «Color propio» / «Degradado propio»
/// si no está.
pub fn skin_name(skin: Skin) -> &'static str {
    let key = SKINS.iter().find(|(s, _)| *s == skin).map_or(
        match skin {
            Skin::Atic => "pill.appearance.skin",
            Skin::Solid { .. } => "pill.appearance.customColor",
            Skin::Gradient { .. } => "pill.appearance.customGradient",
        },
        |(_, name)| *name,
    );
    crate::i18n::t(key)
}

fn file() -> Option<PathBuf> {
    crate::paths::file("appearance.json")
}

fn load() -> Appearance {
    let bytes = file().and_then(|path| std::fs::read(path).ok());
    let mut look: Appearance = bytes.as_deref().and_then(|bytes| serde_json::from_slice(bytes).ok()).unwrap_or_default();
    // Una versión de prueba de macOS puso Liquid Glass en la pill y bajó su
    // tinte en reposo a 15 % (dejó `material` en el archivo): se repone.
    if bytes.as_deref().is_some_and(|bytes| String::from_utf8_lossy(bytes).contains("\"material\"")) {
        if look.rest_tint <= 0.15 {
            look.rest_tint = crate::GLASS_TINT;
        }
        save(&look);
    }
    // Antes el estilo de Atic Code y el de las otras ventanas se elegían
    // aparte: al juntarlos manda el de Atic Code, el que más se usaba.
    if !look.linked {
        let code = crate::code::config::Configs::load();
        look.windows = WindowStyle::of_code(code.style);
        look.glass_opacity = code.glass_opacity.unwrap_or(crate::code::style::DEFAULT_GLASS_OPACITY) as f32 / 100.0;
        look.linked = true;
        save(&look);
    }
    look.rest_tint = look.rest_tint.clamp(REST_RANGE.0, REST_RANGE.1);
    look.read_tint = look.read_tint.clamp(READ_RANGE.0, READ_RANGE.1);
    look.glass_opacity = look.glass_opacity.clamp(0.0, 1.0);
    look.contrast = if look.contrast.is_finite() { look.contrast.clamp(-1.0, 1.0) } else { 0.0 };
    // Solo Material 3 Expressive (`ONLY_EXPRESSIVE`): lo guardado se respeta en el archivo,
    // pero no se aplica.
    if ONLY_EXPRESSIVE {
        look.windows = WindowStyle::Expressive;
    }
    look
}

fn save(look: &Appearance) {
    let Some(path) = file() else {
        return;
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    match serde_json::to_vec_pretty(look) {
        Ok(bytes) => {
            if let Err(error) = std::fs::write(&path, bytes) {
                eprintln!("apariencia: no se pudo guardar {}: {error}", path.display());
            }
        }
        Err(error) => eprintln!("apariencia: {error}"),
    }
}

// --- Sección de los Ajustes ------------------------------------------------------

/// La piel de la pill (`Palette::dark`).
const SKIN: u32 = 0x1a1a18;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Knob {
    Rest,
    Read,
    Glass,
    /// La dirección del degradado propio (0..1 de la barra es 0..360°).
    Angle,
}

impl Knob {
    fn range(self) -> (f32, f32) {
        match self {
            Knob::Rest => REST_RANGE,
            Knob::Read => READ_RANGE,
            Knob::Glass | Knob::Angle => (0.0, 1.0),
        }
    }

    fn get(self, look: &Appearance) -> f32 {
        match self {
            Knob::Rest => look.rest_tint,
            Knob::Read => look.read_tint,
            Knob::Glass => look.glass_opacity,
            Knob::Angle => look.custom.angle as f32 / 360.0,
        }
    }

    fn set(self, look: &mut Appearance, tint: f32) {
        match self {
            Knob::Rest => look.rest_tint = tint,
            Knob::Read => look.read_tint = tint,
            Knob::Glass => look.glass_opacity = tint,
            Knob::Angle => {
                look.custom.angle = (tint * 360.0).round() as u16;
                look.skin = look.custom.gradient();
            }
        }
    }
}

/// El color que se está eligiendo con el selector.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Editing {
    #[default]
    Nothing,
    /// «Color propio».
    Color,
    /// Una parada del degradado propio.
    Gradient(Stop),
    /// Expressive: el acento.
    Accent,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stop {
    From,
    Via,
    To,
}

/// Acentos sugeridos (los de la referencia de M3).
const ACCENTS: [u32; 8] = [0x6750a4, 0x0061a4, 0x006a6a, 0x386a20, 0x8b5000, 0xb3261e, 0x984061, 0x5b5f71];

#[derive(Default)]
pub struct AppearancePane {
    drag: Option<Knob>,
    rest_track: Rc<Cell<Bounds<Pixels>>>,
    read_track: Rc<Cell<Bounds<Pixels>>>,
    glass_track: Rc<Cell<Bounds<Pixels>>>,
    angle_track: Rc<Cell<Bounds<Pixels>>>,
    editing: Editing,
    /// Se arrastró en el selector de color: se guarda al soltar.
    unsaved: bool,
    /// El campo HEX del selector; se crea la primera vez que se abre.
    hex: Option<gpui::Entity<crate::text_input::TextInput>>,
    _hex_changed: Option<gpui::Subscription>,
}

impl AppearancePane {
    fn track(&self, knob: Knob) -> Rc<Cell<Bounds<Pixels>>> {
        match knob {
            Knob::Rest => self.rest_track.clone(),
            Knob::Read => self.read_track.clone(),
            Knob::Glass => self.glass_track.clone(),
            Knob::Angle => self.angle_track.clone(),
        }
    }

    /// Pone el tinte `t` (0..1 de la barra) dentro del rango de la perilla.
    fn set_tint(&mut self, knob: Knob, t: f32, cx: &mut Context<Self>) {
        let (lo, hi) = knob.range();
        let mut look = current();
        knob.set(&mut look, t.clamp(lo, hi));
        apply(look, false);
        if knob == Knob::Glass {
            crate::theme::restyle();
        }
        if knob == Knob::Angle {
            notify_pill(cx);
        }
        cx.notify();
    }

    fn release(&mut self, cx: &mut Context<Self>) {
        let dragged = self.drag.take().is_some();
        if dragged || std::mem::take(&mut self.unsaved) {
            apply(current(), true);
            cx.notify();
        }
    }

    /// El color que muestra el selector, según lo que se edita.
    fn edited(&self, look: &Appearance) -> Option<u32> {
        let custom = look.custom;
        match self.editing {
            Editing::Nothing => None,
            Editing::Color => Some(custom.color),
            Editing::Gradient(Stop::From) => Some(custom.from),
            Editing::Gradient(Stop::Via) => custom.via.or(Some(custom.from)),
            Editing::Gradient(Stop::To) => Some(custom.to),
            Editing::Accent => Some(look.accent.unwrap_or_else(|| crate::pill_look::to_hex(look.skin.mid()))),
        }
    }

    /// Pone `hex` en lo que se edita y lo aplica. Al arrastrar (`persist`
    /// apagado) se guarda al soltar; `from_picker` deja el campo HEX al día.
    fn pick(&mut self, hex: u32, persist: bool, from_picker: bool, cx: &mut Context<Self>) {
        let before = current();
        let mut look = before;
        match self.editing {
            Editing::Nothing => return,
            Editing::Color => {
                look.custom.color = hex;
                look.skin = look.custom.solid();
            }
            Editing::Gradient(stop) => {
                match stop {
                    Stop::From => look.custom.from = hex,
                    Stop::Via => look.custom.via = Some(hex),
                    Stop::To => look.custom.to = hex,
                }
                look.skin = look.custom.gradient();
            }
            Editing::Accent => look.accent = Some(hex),
        }
        // Arrastrando, el selector repite el mismo color: si nada cambia, nada
        // que guardar ni que redibujar.
        if look == before && !persist {
            return;
        }
        apply(look, persist);
        self.unsaved |= !persist;
        if from_picker {
            self.sync_hex(cx);
        }
        notify_pill(cx);
        cx.notify();
    }

    /// Abre el selector de `editing`, o lo cierra si ya estaba abierto (en el
    /// degradado, con cualquiera de sus paradas). «Color propio» y
    /// «Degradado» quedan elegidos de todos modos.
    fn edit(&mut self, editing: Editing, cx: &mut Context<Self>) {
        let open = std::mem::discriminant(&self.editing) == std::mem::discriminant(&editing);
        self.editing = if open { Editing::Nothing } else { editing };
        let mut look = current();
        let skin = match editing {
            Editing::Color => Some(look.custom.solid()),
            Editing::Gradient(_) => Some(look.custom.gradient()),
            _ => None,
        };
        if let Some(skin) = skin.filter(|&skin| skin != look.skin) {
            look.skin = skin;
            apply(look, true);
            cx.refresh_windows();
        }
        self.sync_hex(cx);
        cx.notify();
    }

    fn set_look(change: impl FnOnce(&mut Appearance), cx: &mut gpui::App) {
        let mut look = current();
        change(&mut look);
        apply(look, true);
        // La pill no se redibuja sola en reposo.
        cx.refresh_windows();
    }

    fn sync_hex(&mut self, cx: &mut Context<Self>) {
        let Some(hex) = self.edited(&current()) else {
            return;
        };
        let text = format!("#{hex:06X}");
        if let Some(input) = &self.hex {
            input.update(cx, |input, cx| {
                if input.text() != text {
                    input.set_text(text, cx);
                }
            });
        }
    }

    fn hex_input(&mut self, cx: &mut Context<Self>) -> gpui::Entity<crate::text_input::TextInput> {
        if let Some(input) = &self.hex {
            return input.clone();
        }
        let (text, faint) = (hsla(TEXT), hsla(MUTED));
        let input = cx.new(|cx| crate::text_input::TextInput::new("#RRGGBB", text, faint, text, cx));
        self._hex_changed = Some(cx.subscribe(&input, |pane, input, _: &crate::text_input::Changed, cx| {
            // Escribir un HEX válido lo aplica al tiro.
            if let Some(hex) = crate::color::parse_hex(input.read(cx).text()) {
                pane.pick(hex, true, false, cx);
            }
        }));
        self.hex = Some(input.clone());
        self.sync_hex(cx);
        input
    }

    /// El selector HSV de gpui-m3 con una muestra grande y el campo HEX.
    fn color_editor(&mut self, look: &Appearance, cx: &mut Context<Self>) -> gpui::AnyElement {
        let Some(hex) = self.edited(look) else {
            return div().into_any_element();
        };
        let pane = cx.entity().downgrade();
        let picker = gpui_m3::HsvPicker::new(SharedString::from(format!("appearance-hsv-{:?}", self.editing)), gpui::rgb(hex).into())
            .width(px(240.))
            .on_change(move |color, _, cx| {
                let _ = pane.update(cx, |pane, cx| pane.pick(crate::pill_look::to_hex(color), false, true, cx));
            });
        let input = self.hex_input(cx);
        div()
            .flex()
            .flex_wrap()
            .items_start()
            .gap(px(16.))
            .child(picker)
            .child(
                div()
                    .w(px(132.))
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(div().h(px(56.)).rounded(gpui_m3::corner::M).bg(gpui::rgb(hex)).border_1().border_color(hsla(TRACK)))
                    .child(
                        div()
                            .h(px(36.))
                            .px(px(10.))
                            .rounded(gpui_m3::corner::S)
                            .border_1()
                            .border_color(hsla(TRACK))
                            .flex()
                            .items_center()
                            .type_style(type_scale::BODY_MEDIUM)
                            .child(input),
                    ),
            )
            .into_any_element()
    }

    /// Las muestras de la lista, «Color propio» y «Degradado»: la elegida con
    /// un aro. Las dos últimas llevan un lápiz y abren su selector.
    fn skin_swatches(&self, chosen: Skin, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let ring = crate::theme::styled().map_or_else(|| hsla(FILL), |t| t.accent);
        let look = current();
        // Se dibuja de 30 px, pero el área de clic es de 40 (M3); el aro de foco rodea lo dibujado.
        let swatch = |id: gpui::ElementId, skin: Skin, selected: bool, tip: &'static str, pencil: bool, window: &mut Window, cx: &mut gpui::App| {
            let drawn = div()
                .size(px(30.))
                .p(px(3.))
                .rounded_full()
                .border_2()
                .border_color(if selected { ring } else { gpui::transparent_black() })
                .child(
                    skin.fill_div(div().size_full().rounded_full(), 1.0, None, |el| el.rounded_full())
                        .border_1()
                        .border_color(gpui::white().opacity(0.14)),
                )
                .when(pencil, |el| {
                    el.child(div().absolute().inset_0().flex().items_center().justify_center().child(
                        gpui::svg().path("icons/pencil.svg").size(px(12.)).text_color(gpui::white().opacity(0.9)),
                    ))
                });
            let hit = div()
                .id(id.clone())
                .size(px(40.))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .tooltip(crate::hover::tip(tip));
            crate::settings::focus_target(&id, hit, drawn, gpui_m3::corner::FULL, window, cx)
        };
        let mut swatches = Vec::new();
        for (i, &(skin, name)) in SKINS.iter().enumerate() {
            let el = swatch(("appearance-skin", i).into(), skin, skin == chosen, crate::i18n::t(name), false, window, cx);
            swatches.push(
                el.on_click(cx.listener(move |pane, _: &ClickEvent, _, cx| {
                    if matches!(pane.editing, Editing::Color | Editing::Gradient(_)) {
                        pane.editing = Editing::Nothing;
                    }
                    Self::set_look(|look| look.skin = skin, cx);
                    cx.notify();
                }))
                .into_any_element(),
            );
        }
        let own_color = look.custom.solid();
        let own_gradient = look.custom.gradient();
        let custom = [
            (own_color, chosen == own_color && !chosen.is_preset(), "pill.appearance.customColor", Editing::Color),
            (own_gradient, chosen == own_gradient && !chosen.is_preset(), "pill.appearance.customGradient", Editing::Gradient(Stop::From)),
        ];
        for (i, (skin, selected, name, editing)) in custom.into_iter().enumerate() {
            let el = swatch(("appearance-skin-custom", i).into(), skin, selected, crate::i18n::t(name), true, window, cx);
            swatches.push(el.on_click(cx.listener(move |pane, _: &ClickEvent, _, cx| pane.edit(editing, cx))).into_any_element());
        }
        // Las áreas de clic se tocan: entre lo dibujado quedan 10 px.
        div().flex().flex_wrap().children(swatches)
    }

    /// El degradado propio: la muestra ancha, sus colores (uno elegido para
    /// el selector), agregar o quitar el del medio y la dirección.
    fn gradient_editor(&mut self, look: &Appearance, stop: Stop, window: &mut Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        let custom = look.custom;
        let ring = crate::theme::styled().map_or_else(|| hsla(FILL), |t| t.accent);
        let wide = custom
            .gradient()
            .fill_div(div().h(px(40.)).w_full().rounded(gpui_m3::corner::M), 1.0, None, |el| el.rounded(gpui_m3::corner::M));
        let mut stops = Vec::new();
        for (which, hex, name) in [(Stop::From, Some(custom.from), "pill.appearance.stopFrom"), (Stop::Via, custom.via, "pill.appearance.stopVia"), (Stop::To, Some(custom.to), "pill.appearance.stopTo")] {
            let Some(hex) = hex else { continue };
            let id: gpui::ElementId = SharedString::from(format!("appearance-stop-{which:?}")).into();
            let drawn = div()
                .flex()
                .items_center()
                .gap(px(6.))
                .pl(px(4.))
                .pr(px(10.))
                .py(px(4.))
                .rounded_full()
                .border_1()
                .border_color(if which == stop { ring } else { hsla(TRACK) })
                .child(div().size(px(20.)).rounded_full().bg(gpui::rgb(hex)).border_1().border_color(gpui::white().opacity(0.14)))
                .child(div().text_color(hsla(TEXT)).type_style(type_scale::LABEL_LARGE).child(crate::i18n::t(name)));
            // Se dibuja de 30 px de alto; el área de clic llega a 40 (M3).
            let hit = div().id(id.clone()).py(px(5.)).cursor_pointer();
            stops.push(
                crate::settings::focus_target(&id, hit, drawn, gpui_m3::corner::FULL, window, cx)
                    .on_click(cx.listener(move |pane, _: &ClickEvent, _, cx| {
                        pane.editing = Editing::Gradient(which);
                        pane.sync_hex(cx);
                        cx.notify();
                    }))
                    .into_any_element(),
            );
        }
        let middle = if custom.via.is_some() {
            text_button("appearance-via-remove", crate::i18n::t("pill.appearance.removeVia"), {
                let pane = cx.entity().downgrade();
                move |_, _, cx| {
                    Self::set_look(
                        |look| {
                            look.custom.via = None;
                            look.skin = look.custom.gradient();
                        },
                        cx,
                    );
                    let _ = pane.update(cx, |pane, cx| {
                        pane.editing = Editing::Gradient(Stop::From);
                        pane.sync_hex(cx);
                        cx.notify();
                    });
                }
            })
        } else {
            text_button("appearance-via-add", crate::i18n::t("pill.appearance.addVia"), {
                let pane = cx.entity().downgrade();
                move |_, _, cx| {
                    Self::set_look(
                        |look| {
                            let ends = Skin::Gradient { from: look.custom.from, to: look.custom.to, via: None, angle: 0 };
                            look.custom.via = Some(crate::pill_look::to_hex(ends.mid()));
                            look.skin = look.custom.gradient();
                        },
                        cx,
                    );
                    let _ = pane.update(cx, |pane, cx| {
                        pane.editing = Editing::Gradient(Stop::Via);
                        pane.sync_hex(cx);
                        cx.notify();
                    });
                }
            })
        };
        div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(wide)
            .child(div().flex().flex_wrap().items_center().gap(px(8.)).children(stops).child(middle))
            .child(row(
                crate::i18n::t("pill.appearance.angle"),
                crate::i18n::t("pill.appearance.angleHint"),
                Some(format!("{}°", custom.angle).into()),
                self.slider(Knob::Angle, look, cx),
            ))
            .child(self.color_editor(look, cx))
            .into_any_element()
    }

    /// La tarjeta «Colores»: la piel (lista, propio, degradado) con su
    /// selector y, en Expressive, cómo se usa y el esquema que siembra.
    fn colors(&mut self, look: &Appearance, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let editor = match self.editing {
            Editing::Color => Some(self.color_editor(look, cx)),
            Editing::Gradient(stop) => Some(self.gradient_editor(look, stop, window, cx)),
            _ => None,
        };
        let skin = card()
            .child(row(
                crate::i18n::t("pill.appearance.skin"),
                crate::i18n::t("pill.appearance.skinHint"),
                None,
                self.skin_swatches(look.skin, window, cx),
            ))
            .children(editor);
        let expressive = (look.pill == PillStyle::Expressive).then(|| self.scheme_options(look, cx));
        div().flex().flex_col().gap(px(16.)).child(skin).children(expressive)
    }

    /// Expressive: la piel como fondo o solo semilla, el acento, el estilo de
    /// color (una tarjeta por variante con sus tres colores) y el contraste.
    fn scheme_options(&mut self, look: &Appearance, cx: &mut Context<Self>) -> gpui::AnyElement {
        let key = crate::pill_look::SchemeKey::of(look);
        let cards: Vec<gpui::AnyElement> = SchemeStyle::ALL
            .iter()
            .enumerate()
            .map(|(i, &(style, name))| {
                let s = crate::pill_look::scheme_for(crate::pill_look::SchemeKey { style, ..key });
                let dot = |color: gpui::Hsla| div().size(px(14.)).rounded_full().bg(color);
                div()
                    .flex_1()
                    .min_w_0()
                    .child(
                        gpui_m3::SelectCard::new(("appearance-scheme", i), crate::i18n::t(name))
                            .selected(look.scheme == style)
                            .preview(
                                div()
                                    .h(px(36.))
                                    .rounded(gpui_m3::corner::S)
                                    .bg(s.surface_container)
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .gap(px(6.))
                                    .child(dot(s.primary))
                                    .child(dot(s.secondary))
                                    .child(dot(s.tertiary)),
                            )
                            .on_click(move |_: &ClickEvent, _: &mut Window, cx: &mut gpui::App| {
                                Self::set_look(|look| look.scheme = style, cx)
                            }),
                    )
                    .into_any_element()
            })
            .collect();
        let mut cards = cards.into_iter().peekable();
        let mut grid = Vec::new();
        while cards.peek().is_some() {
            grid.push(div().flex().gap(px(8.)).children(cards.by_ref().take(3)));
        }
        let contrast = CONTRASTS
            .iter()
            .enumerate()
            .min_by(|a, b| (a.1 .0 - look.contrast).abs().total_cmp(&(b.1 .0 - look.contrast).abs()))
            .map_or(0, |(i, _)| i);
        let accent = look.accent.map(|accent| {
            let set = cx.listener(|pane, color: &gpui::Hsla, _, cx| {
                pane.editing = Editing::Nothing;
                Self::set_look(|look| look.accent = Some(crate::pill_look::to_hex(*color)), cx);
                cx.notify();
            });
            div()
                .flex()
                .flex_col()
                .gap(px(10.))
                .child(
                    gpui_m3::ColorSwatches::new("appearance-accent", ACCENTS.map(|hex| gpui::Hsla::from(gpui::rgb(hex))))
                        .current(gpui::rgb(accent).into())
                        .custom(cx.listener(|pane, _: &ClickEvent, _, cx| pane.edit(Editing::Accent, cx)))
                        .on_change(move |color, window, cx| set(&color, window, cx)),
                )
                .when(self.editing == Editing::Accent, |el| el.child(self.color_editor(look, cx)))
        });
        let theme = PillTheme::ALL.iter().position(|&(theme, _)| theme == look.pill_theme).unwrap_or(0);
        card()
            .child(row(
                crate::i18n::t("pill.appearance.pillTheme"),
                crate::i18n::t("pill.appearance.pillThemeHint"),
                None,
                div().flex().child(
                    gpui_m3::SegmentedButtons::new(
                        "appearance-pill-theme",
                        PillTheme::ALL.iter().map(|&(_, name)| gpui_m3::Segment::label(crate::i18n::t(name))).collect::<Vec<_>>(),
                        theme,
                    )
                    .on_change(|index, _, cx| {
                        let theme = PillTheme::ALL.get(index).map_or(PillTheme::Dark, |&(theme, _)| theme);
                        Self::set_look(|look| look.pill_theme = theme, cx);
                    }),
                ),
            ))
            .child(switch_row(
                "appearance-skin-fill",
                crate::i18n::t("pill.appearance.skinFill"),
                crate::i18n::t("pill.appearance.skinFillHint"),
                look.skin_fill,
                |_, _, cx| Self::set_look(|look| look.skin_fill = !look.skin_fill, cx),
            ))
            .child(switch_row(
                "appearance-accent",
                crate::i18n::t("pill.appearance.accent"),
                crate::i18n::t("pill.appearance.accentHint"),
                look.accent.is_some(),
                {
                    let pane = cx.entity().downgrade();
                    move |_, _, cx| {
                        Self::set_look(
                            |look| {
                                look.accent = match look.accent {
                                    Some(_) => None,
                                    None => Some(crate::pill_look::to_hex(look.skin.mid())),
                                }
                            },
                            cx,
                        );
                        let _ = pane.update(cx, |pane, cx| {
                            if pane.editing == Editing::Accent {
                                pane.editing = Editing::Nothing;
                            }
                            cx.notify();
                        });
                    }
                },
            ))
            .children(accent)
            .child(row(
                crate::i18n::t("pill.appearance.schemeStyle"),
                crate::i18n::t("pill.appearance.schemeStyleHint"),
                None,
                div().flex().flex_col().gap(px(8.)).children(grid),
            ))
            .child(row(
                crate::i18n::t("pill.appearance.contrast"),
                crate::i18n::t("pill.appearance.contrastHint"),
                None,
                div().flex().child(
                    gpui_m3::SegmentedButtons::new(
                        "appearance-contrast",
                        CONTRASTS.iter().map(|&(_, name)| gpui_m3::Segment::label(crate::i18n::t(name))).collect::<Vec<_>>(),
                        contrast,
                    )
                    .on_change(|index, _, cx| {
                        let value = CONTRASTS.get(index).map_or(0.0, |&(value, _)| value);
                        Self::set_look(|look| look.contrast = value, cx);
                    }),
                ),
            ))
            .into_any_element()
    }

    /// Una pill y un panel de muestra sobre un fondo claro (una página) y uno
    /// oscuro, con los tintes elegidos. No desenfoca: eso se ve en la pill. Sus colores
    /// son los de la pill y las páginas tal cual, sin pasar por el tema de los Ajustes
    /// (en modo claro, `hsla` volvería oscuro el texto sobre la piel oscura).
    fn preview(&self, look: &Appearance) -> impl IntoElement {
        let pill = crate::theme::raw;
        let tokens = crate::pill_look::PillTokens::of(look, look.pill);
        let (fill, veil, text) = (tokens.fill, tokens.veil, tokens.on);
        let muted = tokens.scheme.map_or(pill(MUTED), |s| s.on_surface_variant);
        let half = |light: bool| {
            let (page, line) = if light { (0xf4f1ea, 0xc9c4b8) } else { (0x22303d, 0x3d5163) };
            let lines = [0.92, 0.78, 0.98, 0.64, 0.86, 0.72, 0.94].into_iter().map(move |w| {
                div()
                    .h(px(6.))
                    .w(gpui::relative(w))
                    .rounded(px(3.))
                    .bg(pill(line))
            });
            let hang = if look.cover_hang { look.read_tint } else { look.rest_tint };
            div()
                .flex_1()
                .min_w_0()
                .relative()
                .overflow_hidden()
                .bg(pill(page))
                .child(
                    div()
                        .absolute()
                        .top(px(14.))
                        .left(px(16.))
                        .right(px(16.))
                        .flex()
                        .flex_col()
                        .gap(px(9.))
                        .children(lines),
                )
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .right_0()
                        .flex()
                        .flex_col()
                        .items_center()
                        // El tab, en reposo.
                        .child(
                            fill.fill_div(div(), look.rest_tint, veil, |el| el.rounded_b(px(12.)))
                                .w(px(96.))
                                .h(px(26.))
                                .rounded_b(px(12.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .gap(px(6.))
                                .type_style(type_scale::LABEL_SMALL)
                                .text_color(text)
                                .child(div().size(px(6.)).rounded_full().bg(pill(AMBER)))
                                .child("1"),
                        )
                        // Lo que cuelga: la letra.
                        .child(
                            fill.fill_div(div(), hang, veil, |el| el.rounded(px(10.)))
                                .mt(px(6.))
                                .px(px(10.))
                                .py(px(4.))
                                .rounded(px(10.))
                                .type_style(type_scale::LABEL_SMALL)
                                .text_color(text)
                                .child(crate::i18n::t("pill.appearance.previewLyric")),
                        )
                        // Un panel abierto.
                        .child(
                            fill.fill_div(div(), look.read_tint, veil, |el| el.rounded(px(12.)))
                                .mt(px(8.))
                                .w(px(150.))
                                .p(px(10.))
                                .rounded(px(12.))
                                .flex()
                                .flex_col()
                                .gap(px(3.))
                                .child(div().type_style(type_scale::LABEL_MEDIUM).text_color(text).child(crate::i18n::t("pill.appearance.previewPanel")))
                                .child(div().type_style(type_scale::LABEL_SMALL).text_color(muted).child(crate::i18n::t("pill.appearance.previewText"))),
                        ),
                )
        };
        div()
            .h(px(172.))
            .flex_none()
            .flex()
            .rounded(gpui_m3::corner::L)
            .overflow_hidden()
            .border_1()
            .border_color(hsla(TRACK))
            .child(half(true))
            .child(half(false))
    }

    /// El deslizador de gpui-m3 a lo ancho de la fila. Al arrastrar queda `drag` puesto: se
    /// guarda al soltar.
    fn slider(&self, knob: Knob, look: &Appearance, cx: &mut Context<Self>) -> gpui::AnyElement {
        let t = knob.get(look).clamp(0.0, 1.0);
        let store = self.track(knob);
        let width = f32::from(store.get().size.width);
        let id: SharedString = format!("appearance-m3-{}", knob as u8).into();
        let pane = cx.entity().downgrade();
        let mut slider = gpui_m3::Slider::new(id, t, 0.0..=1.0)
            .width(px(if width > 0.0 { width } else { 240.0 }))
            .on_change(move |value, _, cx| {
                let _ = pane.update(cx, |pane, cx| {
                    pane.drag = Some(knob);
                    pane.set_tint(knob, value, cx);
                });
            });
        if knob == Knob::Angle {
            slider = slider.step(15.0 / 360.0);
        }
        div()
            .relative()
            .w_full()
            .child(canvas(move |bounds, _, _| store.set(bounds), |_, _, _, _| {}).absolute().size_full())
            .child(slider)
            .into_any_element()
    }
}

/// Las tres tarjetas para elegir el estilo de las ventanas, cada una con una
/// muestra pintada con sus colores; en Liquid Glass, también el deslizador de su
/// transparencia.
fn windows_style(look: Appearance, glass_slider: Option<gpui::AnyElement>) -> impl IntoElement {
    let light = crate::theme::is_light();
    // Con estilo, la elegida con el primario y el radio de las tarjetas del estilo.
    let styled = crate::theme::styled();
    let chosen = styled.as_ref().map_or_else(|| hsla(FILL), |t| t.accent);
    let tile_radius = styled.as_ref().map_or(12., |t| t.r_pop);
    let expressive = crate::theme::expressive();
    let tiles = WindowStyle::ALL.map(|(style, name, hint)| {
        let hint = crate::i18n::t(hint);
        let selected = look.windows == style;
        // Fondo, paneles, acento y radio de la muestra.
        let (bg, pane, accent, radius) = match style.code_style() {
            Some(code) => {
                let t = crate::code::style::tokens(code, light);
                (t.bg, t.raised, t.accent, t.r_chip.min(8.))
            }
            None => {
                let atic = |color| crate::theme::raw(crate::theme::resolve(color));
                (atic(crate::theme::WINDOW), atic(crate::theme::SURFACE_ON), atic(crate::theme::FILL), 4.)
            }
        };
        if let Some(t) = expressive {
            return expressive_style_tile(t, style, name, hint, selected, (bg, pane, accent, radius));
        }
        div()
            .id(SharedString::from(format!("windows-style-{style:?}")))
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .gap(px(8.))
            .p(px(8.))
            .rounded(px(tile_radius))
            .border_2()
            .border_color(if selected { chosen } else { hsla(TRACK) })
            .cursor_pointer()
            .on_click(move |_: &ClickEvent, window: &mut Window, _: &mut gpui::App| reveal(style, window))
            .child(
                div()
                    .h(px(52.))
                    .p(px(6.))
                    .flex()
                    .gap(px(5.))
                    // Concéntrica con la tarjeta: su radio menos el margen y el borde.
                    .rounded(px((tile_radius - 10.).max(4.)))
                    .bg(bg)
                    .child(div().w(px(18.)).rounded(px(radius)).bg(pane))
                    .child(div().flex_1().rounded(px(radius)).bg(pane))
                    .child(div().size(px(14.)).mt_auto().rounded_full().bg(accent)),
            )
            .child(div().text_size(px(13.)).text_color(hsla(TEXT)).child(name))
            .child(div().text_size(px(11.)).text_color(hsla(MUTED)).child(hint))
            .into_any_element()
    });
    card()
        .child(row(
            crate::i18n::t("pill.appearance.windowStyle"),
            crate::i18n::t("pill.appearance.windowStyleHint"),
            None,
            div().flex().gap(px(10.)).children(tiles),
        ))
        .when_some(glass_slider, |el, slider| {
            el.child(row(
                crate::i18n::t("pill.appearance.transparency"),
                crate::i18n::t("pill.appearance.transparencyHint"),
                Some(percent(1.0 - look.glass_opacity)),
                slider,
            ))
        })
}

/// Expressive: la tarjeta de un estilo como tarjeta seleccionable de M3. La
/// elegida va en el contenedor secundario, con contorno primario y esquinas más
/// redondas (28); las demás, con contorno fino, redondean sus esquinas al pasar
/// el cursor y las aprietan al hacer clic. Elegir otra saca a la ventana de
/// Expressive, así que el cambio de elegida no se anima aquí.
fn expressive_style_tile(
    t: crate::code::style::Tokens,
    style: WindowStyle,
    name: &'static str,
    hint: &'static str,
    selected: bool,
    (bg, pane, accent, radius): (gpui::Hsla, gpui::Hsla, gpui::Hsla, f32),
) -> gpui::AnyElement {
    use gpui_m3::corner;
    let (name_color, hint_color) = if selected { (t.on_accent_soft, t.on_accent_soft.opacity(0.8)) } else { (t.text, t.muted) };
    div()
        .id(SharedString::from(format!("windows-style-{style:?}")))
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .gap(px(8.))
        .map(|el| {
            // El mismo tamaño con contorno de 2 o de 1.
            if selected {
                el.p(px(8.)).border_2().border_color(t.accent).bg(t.accent_soft)
            } else {
                el.p(px(9.)).border_1().border_color(t.border)
            }
        })
        .cursor_pointer()
        .on_click(move |_: &ClickEvent, window: &mut Window, _: &mut gpui::App| reveal(style, window))
        .child(
            div()
                .h(px(52.))
                .p(px(6.))
                .flex()
                .gap(px(5.))
                .rounded(if selected { corner::LI } else { corner::S })
                .bg(bg)
                .child(div().w(px(18.)).rounded(px(radius)).bg(pane))
                .child(div().flex_1().rounded(px(radius)).bg(pane))
                .child(div().size(px(14.)).mt_auto().rounded_full().bg(accent)),
        )
        .child(div().text_color(name_color).type_style(type_scale::TITLE_SMALL).child(name))
        .child(div().text_color(hint_color).type_style(type_scale::BODY_SMALL).child(hint))
        .fx(SharedString::from(format!("windows-style-{style:?}-fx")), move |el, h| {
            if selected {
                el.rounded(corner::XL)
            } else {
                let r = f32::from(corner::M) + 8. * h.t - 4. * h.press;
                // `t.hover` en oscuro es el mismo color de la baldosa: no se vería.
                let hover = crate::settings::on_tile_bg(&t);
                el.rounded(px(r)).bg(h.mix(hover.opacity(0.), hover))
            }
        })
        .into_any_element()
}

/// Un cambio de estilo en curso: un círculo con el fondo nuevo crece desde el
/// puntero, como en Atic Code; cuando cubre la ventana se aplica el estilo y el
/// círculo se desvanece.
#[derive(Clone, Copy)]
struct Reveal {
    style: WindowStyle,
    origin: gpui::Point<Pixels>,
    radius: f32,
    color: gpui::Hsla,
    start: std::time::Instant,
    swapped: Option<std::time::Instant>,
}

static REVEAL: Mutex<Option<Reveal>> = Mutex::new(None);
/// Lo que tarda el círculo en desvanecerse con el estilo ya puesto.
const REVEAL_FADE: f32 = 0.2;

/// Cambia el estilo de las ventanas; con movimiento, con el círculo.
fn reveal(style: WindowStyle, window: &mut Window) {
    if current().windows == style {
        // Volviste al estilo de ahora con el círculo de otro creciendo: se
        // cancela (antes seguía y terminaba aplicando el que dejaste).
        if let Ok(mut running) = REVEAL.lock() {
            if running.is_some_and(|r| r.swapped.is_none()) {
                *running = None;
                window.refresh();
            }
        }
        return;
    }
    if crate::code::style::system_reduced_motion() {
        set_windows_style(style);
        window.refresh();
        return;
    }
    let light = crate::theme::is_light();
    // El fondo de la ventana con el estilo nuevo, opaco para que tape al de antes.
    let color = match style.code_style() {
        Some(code) => {
            let t = crate::code::style::tokens(code, light);
            if code == crate::code::style::Style::Glass { t.raised } else { t.pane }
        }
        None => crate::theme::raw(crate::theme::resolve(crate::theme::WINDOW)),
    };
    let origin = window.mouse_position();
    let size = window.viewport_size();
    let (x, y) = (f32::from(origin.x), f32::from(origin.y));
    let (w, h) = (f32::from(size.width), f32::from(size.height));
    let radius = x.max(w - x).hypot(y.max(h - y));
    if let Ok(mut running) = REVEAL.lock() {
        *running = Some(Reveal { style, origin, radius, color, start: std::time::Instant::now(), swapped: None });
    }
    window.refresh();
}

/// La opacidad del vidrio como la guarda Atic Code (0–100, 50 de fábrica):
/// la misma escala que `glass_opacity` (0..1, 0,5).
pub fn code_opacity(glass_opacity: f32) -> u8 {
    (glass_opacity.clamp(0.0, 1.0) * 100.0).round() as u8
}

/// Se eligió un estilo en Atic Code: las demás ventanas lo siguen.
pub fn follow_code_style(code: crate::code::style::Style) {
    let style = WindowStyle::of_code(code);
    if current().windows != style {
        set_windows_style(style);
    }
}

/// Se movió la opacidad del vidrio en Atic Code.
pub fn follow_code_opacity(opacity: u8) {
    let mut look = current();
    if code_opacity(look.glass_opacity) == opacity {
        return;
    }
    look.glass_opacity = opacity as f32 / 100.0;
    apply(look, true);
    crate::theme::restyle();
}

fn set_windows_style(style: WindowStyle) {
    let mut look = current();
    look.windows = style;
    apply(look, true);
    crate::theme::restyle();
}

/// La capa del círculo, al final de la raíz de los Ajustes; no pinta nada sin
/// un cambio en curso.
pub fn reveal_layer(window: &mut Window) -> gpui::AnyElement {
    let Ok(mut running) = REVEAL.lock() else {
        return div().into_any_element();
    };
    let Some(mut reveal) = *running else {
        return div().into_any_element();
    };
    let progress = gpui_m3::Spring::SPATIAL_SLOW.step(reveal.start.elapsed().as_secs_f32());
    let mut opacity = 1.0;
    match reveal.swapped {
        None if progress >= 0.999 => {
            set_windows_style(reveal.style);
            reveal.swapped = Some(std::time::Instant::now());
        }
        None => {}
        Some(at) => {
            let fade = at.elapsed().as_secs_f32() / REVEAL_FADE;
            if fade >= 1.0 {
                *running = None;
                return div().into_any_element();
            }
            opacity = 1.0 - fade;
        }
    }
    *running = Some(reveal);
    window.request_animation_frame();
    let r = (reveal.radius * progress.max(0.0)).max(0.0);
    let viewport = window.viewport_size();
    gpui::deferred(
        gpui::anchored().position(gpui::point(px(0.), px(0.))).child(
            div().w(viewport.width).h(viewport.height).overflow_hidden().child(
                div()
                    .absolute()
                    .left(reveal.origin.x - px(r))
                    .top(reveal.origin.y - px(r))
                    .size(px(r * 2.))
                    .rounded(px(r))
                    .bg(reveal.color.opacity(opacity)),
            ),
        ),
    )
    .with_priority(2)
    .into_any_element()
}

/// Las dos tarjetas para elegir el estilo de la pill, cada una con una pill
/// de muestra pintada con sus colores y la piel elegida.
fn pill_style(look: Appearance) -> impl IntoElement {
    let cards = PillStyle::ALL.iter().enumerate().map(move |(index, &(style, name, hint))| {
        let tokens = crate::pill_look::PillTokens::of(&look, style);
        div().flex_1().min_w_0().child(
            gpui_m3::SelectCard::new(("pill-style", index), name)
                .hint(crate::i18n::t(hint))
                .selected(look.pill == style)
                .preview(pill_sample(&tokens))
                .on_click(move |_: &ClickEvent, window: &mut Window, cx: &mut gpui::App| {
                    let mut look = current();
                    if look.pill == style {
                        return;
                    }
                    look.pill = style;
                    apply(look, true);
                    window.refresh();
                    // La pill no se redibuja sola en reposo.
                    cx.refresh_windows();
                }),
        )
    });
    card().child(row(
        crate::i18n::t("pill.appearance.pillStyle"),
        crate::i18n::t("pill.appearance.pillStyleHint"),
        None,
        div().flex().gap(px(10.)).children(cards),
    ))
}

/// Una pill de muestra con la tira abierta sobre un fondo oscuro: la cara,
/// tres herramientas y, en Expressive, la segunda elegida con su píldora. La
/// piel, el degradado, el velo y el esquema son los elegidos: se ve en vivo.
fn pill_sample(t: &crate::pill_look::PillTokens) -> impl IntoElement {
    let pill = crate::theme::raw;
    let dot = |color: gpui::Hsla| div().size(px(7.)).rounded_full().bg(color);
    let chosen = match t.chosen {
        Some((container, ink)) => div()
            .w(px(20.))
            .h(px(15.))
            .rounded_full()
            .bg(container)
            .flex()
            .items_center()
            .justify_center()
            .child(dot(ink)),
        None => div().w(px(20.)).flex().justify_center().child(dot(t.on)),
    };
    div()
        .h(px(52.))
        .rounded(gpui_m3::corner::S)
        .bg(pill(0x22303d))
        .flex()
        .items_center()
        .justify_center()
        .child(
            t.fill
                .fill_div(div(), 1.0, t.veil, |el| el.rounded_full())
                .h(px(24.))
                .px(px(8.))
                .rounded_full()
                .flex()
                .items_center()
                .gap(px(4.))
                .child(div().size(px(10.)).rounded_full().border_2().border_color(t.face))
                .child(div().w(px(20.)).flex().justify_center().child(dot(t.off)))
                .child(chosen)
                .child(div().w(px(20.)).flex().justify_center().child(dot(t.off))),
        )
}

fn percent(tint: f32) -> SharedString {
    format!("{:.0} %", tint * 100.0).into()
}

impl Render for AppearancePane {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let look = current();
        let backdrop = crate::glass::backdrop_available();
        let toggle = |change: fn(&mut Appearance)| {
            move |_: &ClickEvent, window: &mut Window, _: &mut gpui::App| {
                let mut look = current();
                change(&mut look);
                apply(look, true);
                window.refresh();
            }
        };

        div()
            .flex()
            .flex_col()
            .gap(px(16.))
            // El deslizador de gpui-m3 se mueve solo; aquí solo se suelta.
            .on_mouse_move(cx.listener(|pane, event: &MouseMoveEvent, _, cx| {
                if (pane.drag.is_some() || pane.unsaved) && event.pressed_button != Some(MouseButton::Left) {
                    pane.release(cx);
                }
            }))
            .on_mouse_up(MouseButton::Left, cx.listener(|pane, _: &MouseUpEvent, _, cx| pane.release(cx)))
            .on_mouse_up_out(MouseButton::Left, cx.listener(|pane, _: &MouseUpEvent, _, cx| pane.release(cx)))
            // Con un solo estilo (`ONLY_EXPRESSIVE`) no hay nada que elegir.
            .when(!ONLY_EXPRESSIVE, |el| {
                el.child(windows_style(
                    look,
                    (look.windows == WindowStyle::Glass).then(|| self.slider(Knob::Glass, &look, cx).into_any_element()),
                ))
            })
            .child(pill_style(look))
            .child(div().text_color(hsla(TEXT)).type_style(type_scale::TITLE_MEDIUM).child(crate::i18n::t("pill.appearance.colors")))
            .child(self.colors(&look, window, cx))
            .child(self.preview(&look))
            .child(
                div()
                    .text_color(hsla(MUTED))
                    .type_style(type_scale::BODY_SMALL)
                    .child(crate::i18n::t("pill.appearance.previewNote")),
            )
            .child(
                card()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(8.))
                            .child(switch_row(
                                "appearance-glass",
                                crate::i18n::t("pill.appearance.frostedGlass"),
                                crate::i18n::t("pill.appearance.frostedGlassHint"),
                                look.glass,
                                toggle(|look| look.glass = !look.glass),
                            ))
                            .when(look.glass && !backdrop, |el| {
                                el.child(div().text_color(hsla(AMBER)).type_style(type_scale::BODY_SMALL).child(
                                    if cfg!(target_os = "macos") {
                                        crate::i18n::t("pill.appearance.reduceTransparencyMac")
                                    } else {
                                        crate::i18n::t("pill.appearance.reduceTransparencyWin")
                                    },
                                ))
                            }),
                    )
                    .child(row(
                        crate::i18n::t("pill.appearance.restTint"),
                        crate::i18n::t("pill.appearance.restTintHint"),
                        Some(percent(look.rest_tint)),
                        self.slider(Knob::Rest, &look, cx),
                    )),
            )
            .child(
                card()
                    .child(row(
                        crate::i18n::t("pill.appearance.readTint"),
                        crate::i18n::t("pill.appearance.readTintHint"),
                        Some(percent(look.read_tint)),
                        self.slider(Knob::Read, &look, cx),
                    ))
                    .child(switch_row(
                        "appearance-hang",
                        crate::i18n::t("pill.appearance.coverHang"),
                        crate::i18n::t("pill.appearance.coverHangHint"),
                        look.cover_hang,
                        toggle(|look| look.cover_hang = !look.cover_hang),
                    )),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .child(text_button("appearance-reset", crate::i18n::t("pill.customizeReset"), |_, window, cx| {
                        apply(Appearance::default(), true);
                        window.refresh();
                        cx.refresh_windows();
                    }))
                    .child(div().flex_1())
                    .child(div().text_color(hsla(MUTED)).type_style(type_scale::BODY_SMALL).child(crate::i18n::t("pill.appearance.autoSaved"))),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_pill_parte_opaca_y_sin_vidrio() {
        let look = Appearance::default();
        assert!(!look.glass);
        assert_eq!(look.rest_tint, 1.0);
        assert_eq!(look.read_tint, 1.0);
        // El tinte opaco cabe en el rango del control (no se recorta al cargar).
        assert_eq!(look.rest_tint.clamp(REST_RANGE.0, REST_RANGE.1), 1.0);
        // Un archivo viejo sin estos campos también parte opaco.
        let old: Appearance = serde_json::from_str("{}").unwrap();
        assert!(!old.glass && old.rest_tint == 1.0);
    }

    #[test]
    fn un_appearance_json_viejo_queda_con_la_pill_de_atic() {
        let old: Appearance = serde_json::from_str(r#"{"glass": true, "rest_tint": 0.6, "skin": {"kind": "atic"}}"#).unwrap();
        assert_eq!(old.pill, PillStyle::Atic);
        assert_eq!(Appearance::default().pill, PillStyle::Atic);
        let chosen: Appearance = serde_json::from_str(r#"{"pill": "expressive"}"#).unwrap();
        assert_eq!(chosen.pill, PillStyle::Expressive);
        assert!(serde_json::to_string(&chosen).unwrap().contains(r#""pill":"expressive""#));
    }

    #[test]
    fn un_degradado_viejo_carga_como_el_de_la_lista() {
        let old: Appearance =
            serde_json::from_str(r#"{"skin": {"kind": "gradient", "from": 986153, "to": 3156835}, "pill": "expressive"}"#).unwrap();
        assert_eq!(old.skin, duo(0x0f0c29, 0x302b63));
        assert!(old.skin.is_preset());
        // Lo nuevo queda de fábrica: la piel pinta la pill, esquema tonal y contraste estándar.
        assert!(old.skin_fill);
        assert_eq!((old.scheme, old.contrast, old.accent), (SchemeStyle::TonalSpot, 0.0, None));
        assert_eq!(old.custom, CustomColors::default());
        // Sin color del medio, no se escribe.
        assert!(!serde_json::to_string(&old.skin).unwrap().contains("via"));
    }

    #[test]
    fn los_colores_propios_van_y_vuelven() {
        let look = Appearance {
            skin: Skin::Gradient { from: 0x112233, to: 0xffeedd, via: Some(0x808080), angle: 45 },
            custom: CustomColors { color: 0xabcdef, from: 0x112233, via: Some(0x808080), to: 0xffeedd, angle: 45 },
            accent: Some(0x6750a4),
            scheme: SchemeStyle::FruitSalad,
            contrast: 0.5,
            skin_fill: false,
            pill: PillStyle::Expressive,
            ..Appearance::default()
        };
        let json = serde_json::to_string(&look).unwrap();
        assert!(json.contains(r#""scheme":"fruit_salad""#), "{json}");
        let back: Appearance = serde_json::from_str(&json).unwrap();
        assert!(back == look);
        assert_eq!(back.custom.gradient(), back.skin);
        assert!(!back.skin.is_preset());
    }

    #[test]
    fn el_tema_de_la_pill_sigue_a_las_ventanas_si_no_se_eligio() {
        // Una apariencia guardada antes de poder elegirlo va con las ventanas.
        let old: Appearance = serde_json::from_str(r#"{"pill":"expressive"}"#).unwrap();
        assert_eq!(old.pill_theme, PillTheme::System);
        let look = Appearance { pill_theme: PillTheme::System, ..Appearance::default() };
        let json = serde_json::to_string(&look).unwrap();
        assert!(json.contains(r#""pill_theme":"system""#), "{json}");
        assert!(serde_json::from_str::<Appearance>(&json).unwrap() == look);
        // Oscuro y claro no miran el sistema; «Seguir al sistema», sí.
        for system_light in [false, true] {
            assert!(PillTheme::Dark.dark(system_light));
            assert!(!PillTheme::Light.dark(system_light));
            assert_eq!(PillTheme::System.dark(system_light), !system_light);
        }
    }

    #[test]
    fn un_degradado_de_tres_va_en_dos_capas_si_es_opaco() {
        let three = Skin::Gradient { from: 0x000000, to: 0xffffff, via: Some(0xff0000), angle: 90 };
        assert!(three.layers(1.0).1.is_some());
        // Translúcido, una sola capa (dos taparían más en la segunda mitad).
        assert!(three.layers(0.6).1.is_none());
        assert!(duo(0x0f2027, 0x2c5364).layers(1.0).1.is_none());
        assert_eq!(three.mid(), gpui::Hsla::from(gpui::rgb(0xff0000)));
        assert_eq!(three.stops().len(), 3);
        assert_eq!(Skin::Solid { color: 0x123456 }.stops().len(), 1);
    }

    #[test]
    fn en_atic_ninguna_piel_de_la_lista_cambia() {
        // Todas son oscuras: el texto claro se lee sin velo, como antes.
        for (skin, _) in SKINS {
            assert!(crate::pill_look::PillTokens::atic(skin).veil.is_none(), "{skin:?}");
        }
    }

    #[test]
    fn el_angulo_propio_da_la_vuelta() {
        let custom = CustomColors { angle: 360, ..CustomColors::default() };
        assert_eq!(custom.gradient(), Skin::Gradient { from: custom.from, to: custom.to, via: None, angle: 0 });
    }
}
