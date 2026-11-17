//! El aspecto de la pill según su estilo (`appearance::PillStyle`).
//!
//! La pill se pinta a mano (`Frame::paint_base` y `paint_top` en `main.rs`):
//! los componentes de gpui-m3 no entran en ese lienzo. Lo que cambia entre un
//! estilo y otro vive en `PillTokens`, que `frame` resuelve una vez por cuadro:
//!
//! - **Atic**: las constantes de siempre, tal cual (`PillTokens::atic`).
//! - **M3 Expressive** («toolbar flotante»): los roles de un `Scheme`
//!   sembrado con la piel elegida (la de Atic usa `baseline_dark` o
//!   `baseline_light`), oscuro o claro según «Tema de la pill»
//!   (`Appearance::pill_theme`); fundida con el notch, siempre oscuro. La tira
//!   es un grupo de botones conectado (`group_layout`, `group_radii`): cada
//!   ficha con su fondo tonal, la del cursor más ancha (sus vecinas ceden, el
//!   largo no cambia), más cuadrada al presionar y la elegida rellena y
//!   redonda. La cara lleva un contenedor cuya forma dice el estado
//!   (`mark_shape`).
//!
//! La piel (lisa, o un degradado de dos o tres colores) pinta la pill en los
//! dos estilos (`Skin::paint`); si es clara, lleva encima un velo hasta que el
//! contenido se lea (`veil_for`). En Expressive también puede ser solo la
//! semilla del esquema (`Appearance::skin_fill`), y el esquema sigue el estilo
//! de color, el contraste y el acento elegidos (`SchemeKey`).
//!
//! `LookState` guarda lo que se anima solo en Expressive (el ancho y la
//! elegida de cada ficha, el radio al presionar y la forma de la cara), con
//! resortes de gpui-m3; `Pill::advance` lo avanza. En Atic no hace nada.

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use gpui::{point, px, rgb, BoxShadow, Hsla, PathBuilder, Window};
use gpui_m3::shapes::LOADING_SEQUENCE;
use gpui_m3::{Elevation, Morph, Scheme, SchemeVariant, ShapeName, Spring};

use crate::anim::{SpringKind, SpringTrack};
use crate::appearance::{Appearance, PillStyle, SchemeStyle, Skin};
use crate::pill_tools::STRIP_MAX;
use crate::tweak::num;

/// Opacidad de la capa de estado al pasar el cursor y al presionar (M3).
const HOVER_LAYER: f32 = 0.08;
const PRESS_LAYER: f32 = 0.10;
/// La tira como grupo de botones conectado (M3 Expressive): cada ficha es un
/// segmento de este grueso, con esquinas chicas entre fichas y redondas en los
/// extremos del grupo.
const GROUP_THICK: f32 = 32.0;
const GROUP_INNER: f32 = 8.0;
/// El radio de todas sus esquinas mientras se presiona.
const GROUP_PRESSED: f32 = 4.0;
/// Lo que se ensancha la ficha con el cursor encima; lo ceden sus vecinas.
const GROUP_GROW: f32 = 12.0;
/// Íconos de la tira en Expressive (en Atic, `TOOL_ICON`).
const M3_ICON: f32 = 20.0;
/// El contenedor de la cara.
pub(crate) const FACE_BOX: f32 = 32.0;
/// Lo que dura el «hecho» (Cookie4) antes de volver a reposo.
const DONE_MS: u64 = 420;
/// La cara «procesando» recorre `LOADING_SEQUENCE` a estos cuadros por
/// segundo (como `LoadingIndicator`): se lee continuo y no redibuja de más.
const LOADING_FPS: f32 = 15.0;
/// Cuando lo único que procesa es un agente (puede durar horas): 10 cuadros
/// por segundo, justo dos sondeos en calma de `Pill::tick` (50 ms). A 15 los
/// cuadros caían a saltos de 50 y 100 ms.
const LOADING_FPS_AGENTS: f32 = 10.0;

/// Expressive: con la piel pintada, el fondo de cada ficha de la tira es una
/// capa del texto sobre la piel (así el degradado se ve entre y bajo las fichas).
const SEGMENT_ON_SKIN: f32 = 0.10;
/// Contraste mínimo del texto (principal y secundario) sobre la piel (WCAG).
const TEXT_CONTRAST: f32 = 4.5;
/// La semilla de `Scheme::baseline_dark`: la de la piel de Atic con otra
/// variante o contraste.
const BASELINE_SEED: u32 = 0x6750a4;

/// Los colores, tamaños y sombras de la pill en un estilo.
#[derive(Clone)]
pub(crate) struct PillTokens {
    /// La piel para los contornos: lisa o en degradado.
    pub fill: Skin,
    /// Una capa translúcida sobre la piel cuando el texto claro no se
    /// leería encima (una piel clara elegida a mano); `None` casi siempre.
    pub veil: Option<Hsla>,
    /// La piel en un solo color (lo que no cabe en un degradado).
    pub skin: Hsla,
    /// La cara, sin contenedor.
    pub face: Hsla,
    /// Íconos de la tira: apagado y con el cursor encima.
    pub off: Hsla,
    pub on: Hsla,
    /// Cuánto crece el ícono con el cursor encima (0 en Expressive: ahí lo
    /// dice la capa de estado).
    pub hover_grow: f32,
    pub icon: f32,
    /// Color de la capa de estado (sin opacidad); `None` en Atic.
    pub layer: Option<Hsla>,
    /// El fondo de cada ficha del grupo de la tira; `None` en Atic.
    pub segment: Option<Hsla>,
    /// La ficha elegida: contenedor y contenido.
    pub chosen: Option<(Hsla, Hsla)>,
    /// El reloj de la grabación en un contenedor: contenedor y contenido.
    pub recording: Option<(Hsla, Hsla)>,
    /// Otro reloj del tab (Pomodoro, Computadores) en un contenedor.
    pub clock: Option<(Hsla, Hsla)>,
    /// El rótulo de la tira como tooltip de M3: fondo y texto.
    pub tooltip: Option<(Hsla, Hsla)>,
    pub scheme: Option<Scheme>,
    shadow: Shadow,
}

#[derive(Clone)]
enum Shadow {
    /// `pill_shadow` de siempre.
    Atic,
    /// Compartidas: copiar los tokens no copia la lista.
    Elevation(Arc<[BoxShadow]>),
}

/// Lo que decide los tokens de `PillTokens::current`: el estilo, el esquema
/// (con la piel) y si la piel se pinta.
type TokensKey = (PillStyle, SchemeKey, bool);

/// Los últimos tokens de `PillTokens::current`: la pill los pide varias veces
/// por cuadro y armarlos busca el esquema y calcula el velo.
static TOKENS: Mutex<Option<(TokensKey, PillTokens)>> = Mutex::new(None);

/// El valor guardado en `slot` si es de `key`; si no, lo arma `make` y lo
/// guarda en su lugar.
fn remember<K: PartialEq + Copy, V: Clone>(slot: &Mutex<Option<(K, V)>>, key: K, make: impl FnOnce() -> V) -> V {
    if let Ok(slot) = slot.lock() {
        if let Some((seen, value)) = slot.as_ref() {
            if *seen == key {
                return value.clone();
            }
        }
    }
    let value = make();
    if let Ok(mut slot) = slot.lock() {
        *slot = Some((key, value.clone()));
    }
    value
}

/// El ícono de la tira en Expressive (se puede afinar en `tweaks.json`).
fn m3_icon() -> f32 {
    num("pill.m3.icon", M3_ICON)
}

impl PillTokens {
    /// Los del estilo elegido en Apariencia. Se guardan mientras no cambie lo
    /// que los decide (`TokensKey`).
    pub fn current() -> Self {
        let look = crate::appearance::current();
        let scheme = SchemeKey::of_pill(&look);
        let key = (look.pill, scheme, look.skin_fill);
        let mut tokens = remember(&TOKENS, key, || match look.pill {
            PillStyle::Atic => Self::atic(look.skin),
            PillStyle::Expressive => Self::expressive_with(scheme, look.skin_fill),
        });
        // Lo único que depende de `tweaks.json`: al día aunque esté guardado.
        if tokens.scheme.is_some() {
            tokens.icon = m3_icon();
        }
        tokens
    }

    /// Los de `style` con los colores de `look` (la muestra de cada estilo
    /// en Apariencia).
    pub fn of(look: &Appearance, style: PillStyle) -> Self {
        match style {
            PillStyle::Atic => Self::atic(look.skin),
            PillStyle::Expressive => Self::expressive_with(SchemeKey::of(look), look.skin_fill),
        }
    }

    /// El estilo de siempre: exactamente las constantes de antes de que
    /// hubiera estilos (`Palette::dark`, `pill_shadow`, `TOOL_ICON`). Una
    /// piel propia clara lleva encima un velo de la de Atic.
    pub fn atic(skin: Skin) -> Self {
        let text: Hsla = rgb(0xf0f0ea).into();
        Self {
            fill: skin,
            veil: veil_for(&skin.stops(), Skin::Atic.mid(), &[(text, TEXT_CONTRAST)]),
            skin: skin.mid(),
            face: text,
            off: rgb(0xa8a89e).into(),
            on: text,
            hover_grow: 0.14,
            icon: crate::TOOL_ICON,
            layer: None,
            segment: None,
            chosen: None,
            recording: None,
            clock: None,
            tooltip: None,
            scheme: None,
            shadow: Shadow::Atic,
        }
    }

    /// Material 3 Expressive, desde un esquema oscuro sembrado con la piel,
    /// con la piel pintada y lo demás de fábrica.
    #[cfg(test)]
    pub fn expressive(skin: Skin) -> Self {
        Self::expressive_with(SchemeKey::plain(skin), true)
    }

    /// Material 3 Expressive con el esquema de `key`. Con `paint`, la piel
    /// (lisa o en degradado) es el fondo de la pill y las fichas de la tira
    /// son capas sobre ella; si no, o con la piel de Atic, la pill va en el
    /// contenedor del esquema. El esquema es oscuro o claro según el tema
    /// (`SchemeKey::dark`): una piel que no contrasta con su texto (clara con
    /// tema oscuro, oscura con tema claro) lleva encima un velo de `surface`
    /// hasta que el texto y los íconos se lean.
    pub fn expressive_with(key: SchemeKey, paint: bool) -> Self {
        let s = scheme_for(key);
        let (fill, skin, veil, segment) = if paint && key.skin != Skin::Atic {
            let veil = veil_for(
                &key.skin.stops(),
                s.surface,
                // `on_surface_variant` también es texto (secundario): 4,5:1 como el principal.
                &[(s.on_surface, TEXT_CONTRAST), (s.on_surface_variant, TEXT_CONTRAST)],
            );
            let mid = key.skin.mid();
            let skin = veil.map_or(mid, |veil| blend(mid, Hsla { a: 1.0, ..veil }, veil.a));
            (key.skin, skin, veil, s.on_surface.opacity(SEGMENT_ON_SKIN))
        } else {
            (Skin::Solid { color: to_hex(s.surface_container) }, s.surface_container, None, s.surface_container_high)
        };
        Self {
            fill,
            veil,
            skin,
            face: s.on_surface,
            off: s.on_surface_variant,
            on: s.on_surface,
            hover_grow: 0.0,
            icon: m3_icon(),
            layer: Some(s.on_surface),
            segment: Some(segment),
            chosen: Some((s.secondary_container, s.on_secondary_container)),
            recording: Some((s.error_container, s.on_error_container)),
            clock: Some((s.surface_container_highest, s.on_surface)),
            tooltip: Some((s.inverse_surface, s.inverse_on_surface)),
            scheme: Some(s),
            shadow: Shadow::Elevation(Elevation::Level3.shadow(&s).into()),
        }
    }

    /// La sombra de la pill, de los paneles y de la rueda, a `alpha`.
    pub fn shadow(&self, alpha: f32) -> Vec<BoxShadow> {
        match &self.shadow {
            Shadow::Atic => vec![crate::pill_shadow(alpha)],
            Shadow::Elevation(shadows) => shadows
                .iter()
                .map(|shadow| BoxShadow { color: shadow.color.opacity(alpha), ..shadow.clone() })
                .collect(),
        }
    }

    /// La opacidad de la capa de estado con `hover` y `press` (0..1).
    pub fn layer_alpha(hover: f32, press: f32) -> f32 {
        let hover = hover.clamp(0.0, 1.0);
        let press = press.clamp(0.0, 1.0);
        num("pill.m3.hoverLayer", HOVER_LAYER) * hover * (1.0 - press) + num("pill.m3.pressLayer", PRESS_LAYER) * press
    }

    /// El color de una ficha del grupo: su fondo tonal, el de la elegida
    /// según `chosen` y encima la capa de estado. `None` en Atic.
    pub fn segment_color(&self, hover: f32, press: f32, chosen: f32) -> Option<Hsla> {
        let base = self.segment?;
        let fill = match self.chosen {
            Some((container, _)) => blend(base, container, chosen),
            None => base,
        };
        Some(match self.layer {
            Some(layer) => blend(fill, layer, Self::layer_alpha(hover, press)),
            None => fill,
        })
    }
}

// --- La tira como grupo conectado (Expressive) ----------------------------

/// El grueso de cada ficha del grupo.
pub(crate) fn group_thick() -> f32 {
    num("pill.m3.group.thick", GROUP_THICK)
}

/// Lo que se ensancha la ficha con el cursor encima.
pub(crate) fn group_grow() -> f32 {
    num("pill.m3.group.grow", GROUP_GROW)
}

/// Dónde empieza cada ficha y cuánto mide a lo largo, desde el principio de
/// la tira abierta del todo. `widen` (0..1 por ficha; el resorte puede pasarse
/// un poco) dice cuánto se ensancha cada una: crece `grow * widen` y sus
/// vecinas ceden lo mismo, mitad y mitad (todo la única vecina en un extremo).
/// El largo del grupo no cambia nunca: el tab no se mueve con el hover.
pub(crate) fn group_layout(count: usize, base: f32, gap: f32, grow: f32, widen: &[f32]) -> Vec<(f32, f32)> {
    let mut widths = vec![base; count];
    // Que una vecina que cede por los dos lados no quede más angosta que el ícono.
    let grow = grow.clamp(0.0, base * 0.3);
    if count > 1 && grow > 0.0 {
        for index in 0..count {
            let extra = grow * widen.get(index).copied().unwrap_or(0.0).clamp(-0.25, 1.25);
            if extra == 0.0 {
                continue;
            }
            let left = index.checked_sub(1);
            let right = (index + 1 < count).then_some(index + 1);
            let share = extra / (left.is_some() as u8 + right.is_some() as u8) as f32;
            widths[index] += extra;
            for neighbour in left.into_iter().chain(right) {
                widths[neighbour] -= share;
            }
        }
    }
    let mut start = 0.0;
    widths
        .into_iter()
        .map(|width| {
            let segment = (start, width);
            start += width + gap;
            segment
        })
        .collect()
}

/// La ficha bajo `along` (medido desde el principio de la tira) con la tira
/// abierta en `open`. Cada una se lleva la mitad del espacio hasta sus
/// vecinas: entre dos no queda hueco muerto. Con todas iguales es la cuenta
/// de siempre (centro ± (ancho + espacio) / 2).
pub(crate) fn group_slot_at(layout: &[(f32, f32)], gap: f32, open: f32, along: f32) -> Option<usize> {
    layout
        .iter()
        .position(|&(start, width)| (along - (start + width / 2.0) * open).abs() <= (width + gap) / 2.0)
}

/// Los radios de una ficha del grupo, al principio y al final a lo largo:
/// redondas en los extremos del grupo y chicas entre fichas; la elegida,
/// redonda entera (`chosen`); al presionar, todas más cuadradas (`press`).
pub(crate) fn group_radii(index: usize, count: usize, thick: f32, chosen: f32, press: f32) -> (f32, f32) {
    let full = thick / 2.0;
    let inner = num("pill.m3.group.inner", GROUP_INNER).min(full);
    let pressed = num("pill.m3.group.pressed", GROUP_PRESSED).min(full);
    let side = |outer: bool| {
        let rest = if outer { full } else { inner };
        let rest = rest + (full - rest) * chosen.clamp(0.0, 1.0);
        rest + (pressed - rest) * press.clamp(0.0, 1.0)
    };
    (side(index == 0), side(index + 1 == count))
}

/// Una ficha del grupo de la tira, lista para pintar. `corners`: arriba a la
/// izquierda, arriba a la derecha, abajo a la derecha, abajo a la izquierda.
pub(crate) struct StripSegment {
    pub rect: crate::geometry::Rect,
    pub corners: [f32; 4],
    pub color: Hsla,
}

/// Lo que decide el esquema de Expressive.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SchemeKey {
    pub skin: Skin,
    /// La semilla, si no es la piel.
    pub accent: Option<u32>,
    pub style: SchemeStyle,
    /// -1..1; 0 es el estándar.
    pub contrast: f32,
    /// Oscuro o claro (`Appearance::pill_theme`).
    pub dark: bool,
}

impl SchemeKey {
    /// El de lo elegido en Apariencia (la muestra de Ajustes).
    pub fn of(look: &Appearance) -> Self {
        Self {
            skin: look.skin,
            accent: look.accent,
            style: look.scheme,
            contrast: look.contrast,
            // El tema de las ventanas (`theme::is_light`, que ya sigue al sistema si
            // las ventanas lo siguen): cambia en el acto, junto con ellas.
            dark: look.pill_theme.dark(crate::theme::is_light()),
        }
    }

    /// El de la pill: como `of`, pero oscuro mientras la pill lo pide
    /// (`set_force_dark`).
    pub fn of_pill(look: &Appearance) -> Self {
        let key = Self::of(look);
        Self { dark: key.dark || FORCE_DARK.load(Ordering::Relaxed), ..key }
    }

    /// La piel con el estilo de color y el contraste de fábrica, oscura.
    #[cfg(test)]
    pub fn plain(skin: Skin) -> Self {
        Self { skin, accent: None, style: SchemeStyle::TonalSpot, contrast: 0.0, dark: true }
    }
}

/// El claro u oscuro del sistema, para «Seguir al sistema»: `UNSEEN` hasta
/// que se mira por primera vez; después lo pone al día `config_watch`.
static SYSTEM_LIGHT: AtomicU8 = AtomicU8::new(UNSEEN);
const UNSEEN: u8 = u8::MAX;

/// La pill va oscura aunque el tema sea claro (`set_force_dark`).
static FORCE_DARK: AtomicBool = AtomicBool::new(false);

/// Anota el claro u oscuro del sistema; `true` si cambió.
pub(crate) fn set_system_light(light: bool) -> bool {
    SYSTEM_LIGHT.swap(light as u8, Ordering::Relaxed) != light as u8
}

/// La pill lo anota al dibujarse: va oscura en cualquier tema fundida con el
/// notch de la cámara (ahí la piel es negra) y con Ahora suena sobre la
/// carátula (su texto claro va sobre el degradado oscuro de la imagen).
pub(crate) fn set_force_dark(on: bool) {
    FORCE_DARK.store(on, Ordering::Relaxed);
}

/// `from_seed_variant` hace cuentas de color (HCT): se guardan los últimos
/// esquemas (la pill, y en Apariencia la muestra de cada estilo de color).
static SCHEMES: Mutex<Vec<(SchemeKey, Scheme)>> = Mutex::new(Vec::new());
const SCHEMES_KEPT: usize = 16;

pub(crate) fn scheme_for(key: SchemeKey) -> Scheme {
    if let Ok(mut cached) = SCHEMES.lock() {
        if let Some(scheme) = lru_get(&mut cached, &key) {
            return scheme;
        }
    }
    let scheme = build_scheme(key);
    if let Ok(mut cached) = SCHEMES.lock() {
        lru_put(&mut cached, key, scheme, SCHEMES_KEPT);
    }
    scheme
}

/// Lo guardado para `key`, y lo deja al final: lo que se sigue usando es lo
/// último que se bota.
fn lru_get<K: PartialEq, V: Copy>(entries: &mut Vec<(K, V)>, key: &K) -> Option<V> {
    let index = entries.iter().position(|(seen, _)| seen == key)?;
    let value = entries[index].1;
    if index + 1 != entries.len() {
        let entry = entries.remove(index);
        entries.push(entry);
    }
    Some(value)
}

/// Guarda `value` al final; lleno, bota el que lleva más tiempo sin usarse
/// (el primero).
fn lru_put<K, V>(entries: &mut Vec<(K, V)>, key: K, value: V, kept: usize) {
    if entries.len() >= kept.max(1) {
        entries.remove(0);
    }
    entries.push((key, value));
}

fn build_scheme(key: SchemeKey) -> Scheme {
    let standard = key.style == SchemeStyle::TonalSpot && key.contrast == 0.0;
    let seed: Hsla = match (key.accent, key.skin) {
        (Some(accent), _) => rgb(accent).into(),
        (None, Skin::Atic) if standard && key.dark => return Scheme::baseline_dark(),
        (None, Skin::Atic) if standard => return Scheme::baseline_light(),
        (None, Skin::Atic) => rgb(BASELINE_SEED).into(),
        (None, skin) => skin.mid(),
    };
    // Negro, gris o blanco no tienen matiz: con TonalSpot salían marrones
    // o morados sacados de la nada. Sin color, el esquema va en grises.
    let hueless = seed.s < 0.12 || seed.l < 0.06 || seed.l > 0.94;
    let variant = match key.style {
        SchemeStyle::TonalSpot if hueless => SchemeVariant::Monochrome,
        style => style.variant(),
    };
    Scheme::from_seed_variant(seed, key.dark, variant, key.contrast)
}

/// La luminancia relativa de WCAG (0 negro, 1 blanco).
fn luminance(color: Hsla) -> f32 {
    let c = gpui::Rgba::from(color);
    let lin = |v: f32| if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) };
    0.2126 * lin(c.r) + 0.7152 * lin(c.g) + 0.0722 * lin(c.b)
}

/// La razón de contraste de WCAG entre dos colores opacos (1 a 21).
fn contrast_ratio(a: Hsla, b: Hsla) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// La capa de `under` que hay que poner sobre una piel de colores `stops`
/// para que cada tinta llegue a su contraste mínimo en todos ellos; `None`
/// si ya llegan sin capa. Sube de a 5 % (opaca del todo como último caso).
fn veil_for(stops: &[Hsla], under: Hsla, inks: &[(Hsla, f32)]) -> Option<Hsla> {
    let reads = |alpha: f32| {
        stops.iter().all(|&stop| {
            let bg = blend(stop, under, alpha);
            inks.iter().all(|&(ink, min)| contrast_ratio(ink, bg) >= min)
        })
    };
    if reads(0.0) {
        return None;
    }
    let alpha = (1..=20).map(|step| step as f32 * 0.05).find(|&alpha| reads(alpha)).unwrap_or(1.0);
    Some(Hsla { a: alpha, ..under })
}

pub(crate) fn to_hex(color: Hsla) -> u32 {
    let c = gpui::Rgba::from(color);
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u32;
    (byte(c.r) << 16) | (byte(c.g) << 8) | byte(c.b)
}

/// Mezcla en sRGB: `t` 0 es `from`, 1 es `to`.
fn blend(from: Hsla, to: Hsla, t: f32) -> Hsla {
    gpui_m3::theme::mix(to, from, t.clamp(0.0, 1.0))
}

// --- La cara ----------------------------------------------------------------

/// Lo que dice la cara con su contenedor, de lo más a lo menos importante.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MarkState {
    Rest,
    Hover,
    /// El notch abierto.
    Open,
    Dictating,
    Recording,
    /// Algo trabaja sin que haya que mirarlo: se transcribe, se resume o un
    /// agente está en su turno. La forma recorre `LOADING_SEQUENCE`.
    Processing,
    /// Hay una actualización.
    Attention,
    /// Algo terminó (el dictado se pegó, la grabación se detuvo, terminó una
    /// transcripción, se guardó una captura).
    Done,
}

/// El rol de color del contenedor de la cara.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Role {
    /// Sin contenedor.
    Hidden,
    Highest,
    Secondary,
    Primary,
    Error,
    Tertiary,
}

impl Role {
    /// Contenedor y contenido (la cara encima).
    fn colors(self, s: &Scheme) -> (Hsla, Hsla) {
        match self {
            Role::Hidden => (s.surface_container_highest.opacity(0.0), s.on_surface),
            Role::Highest => (s.surface_container_highest, s.on_surface),
            Role::Secondary => (s.secondary_container, s.on_secondary_container),
            Role::Primary => (s.primary_container, s.on_primary_container),
            Role::Error => (s.error_container, s.on_error_container),
            Role::Tertiary => (s.tertiary_container, s.on_tertiary_container),
        }
    }
}

/// El estado de la cara con lo que pasa ahora, en orden de importancia.
pub(crate) fn mark_state(
    recording: bool,
    dictating: bool,
    processing: bool,
    done: bool,
    attention: bool,
    open: bool,
    hover: bool,
) -> MarkState {
    if recording {
        MarkState::Recording
    } else if dictating {
        MarkState::Dictating
    } else if processing {
        MarkState::Processing
    } else if done {
        MarkState::Done
    } else if attention {
        MarkState::Attention
    } else if open {
        MarkState::Open
    } else if hover {
        MarkState::Hover
    } else {
        MarkState::Rest
    }
}

/// La forma y el color del contenedor de la cara en cada estado.
pub(crate) fn mark_shape(state: MarkState) -> (ShapeName, Role) {
    match state {
        MarkState::Rest => (ShapeName::Circle, Role::Hidden),
        MarkState::Hover => (ShapeName::Cookie9, Role::Highest),
        MarkState::Open => (ShapeName::Square, Role::Secondary),
        MarkState::Dictating => (ShapeName::Sunny, Role::Primary),
        MarkState::Recording => (ShapeName::SoftBurst, Role::Error),
        // La primera de `LOADING_SEQUENCE`: desde ahí sigue la secuencia
        // (`LookState::mark_points`). Con movimiento reducido, quieta en Circle.
        MarkState::Processing => (LOADING_SEQUENCE[0], Role::Secondary),
        MarkState::Attention => (ShapeName::Clover4, Role::Tertiary),
        MarkState::Done => (ShapeName::Cookie4, Role::Primary),
    }
}

/// El contenedor de la cara en este cuadro: sus puntos en la caja unidad
/// (0..1) y su color. La cara encima va en `ink`.
#[derive(Clone)]
pub(crate) struct MarkPaint {
    pub points: Vec<(f32, f32)>,
    pub color: Hsla,
    pub ink: Hsla,
}

impl MarkPaint {
    /// Pinta el contenedor centrado en `center`, de lado `side`.
    pub fn paint(&self, window: &mut Window, center: (f32, f32), side: f32, alpha: f32) {
        let color = self.color.opacity(alpha);
        if color.a <= 0.01 {
            return;
        }
        let points: Vec<_> = self
            .points
            .iter()
            .map(|&(x, y)| point(px(center.0 + (x - 0.5) * side), px(center.1 + (y - 0.5) * side)))
            .collect();
        let mut builder = PathBuilder::fill();
        builder.add_polygon(&points, true);
        if let Ok(path) = builder.build() {
            window.paint_path(path, color);
        }
    }
}

/// El lado del contenedor para una cara de `mark_size` (la cara se achica en
/// el notch cerrado según `tab_size`).
pub(crate) fn face_box(mark_size: f32) -> f32 {
    num("pill.m3.faceBox", FACE_BOX) * mark_size / crate::MARK_SIZE
}

// --- Lo que se anima --------------------------------------------------------

/// Un valor que sigue a su objetivo con un resorte de gpui-m3: se puede
/// redirigir a mitad de camino sin perder la velocidad.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Track {
    pub value: f32,
    velocity: f32,
    target: f32,
}

impl Track {
    pub const fn at(value: f32) -> Self {
        Self { value, velocity: 0.0, target: value }
    }

    pub fn set(&mut self, target: f32) {
        self.target = target;
    }

    pub fn snap(&mut self, value: f32) {
        *self = Self::at(value);
    }

    pub fn moving(&self) -> bool {
        (self.value - self.target).abs() > 1e-3 || self.velocity.abs() > 1e-2
    }

    fn step(&mut self, spring: Spring, dt: f32) {
        if !self.moving() {
            self.snap(self.target);
            return;
        }
        let (x, v) = spring.solve(self.value - self.target, self.velocity, dt);
        self.value = self.target + x;
        self.velocity = v;
        if !self.moving() {
            self.snap(self.target);
        }
    }
}

/// Lo que mira `LookState::update` en cada cuadro.
#[derive(Clone, Copy)]
pub(crate) struct LookInputs {
    /// La ficha de la herramienta cuyo vistazo o panel está abierto.
    pub chosen: Option<usize>,
    /// La ficha apretada.
    pub pressed: Option<usize>,
    /// Hacia dónde va el ancho de cada ficha del grupo (1: con el cursor
    /// encima o con su vistazo abierto). Es el objetivo, el mismo que usa el
    /// hit-test (`Pill::strip_layout`).
    pub widen: [f32; STRIP_MAX],
    pub recording: bool,
    pub dictating: bool,
    /// Se transcribe, se resume o un agente está en su turno.
    pub processing: bool,
    /// Lo único que procesa es un agente (no el dictado ni Reuniones): la
    /// cara va a `LOADING_FPS_AGENTS`.
    pub agents_only: bool,
    /// Lo que estaba en marcha terminó mal: sin «hecho».
    pub failed: bool,
    pub attention: bool,
    pub open: bool,
    pub hover: bool,
    pub reduced: bool,
}

impl LookInputs {
    /// Nada pasa.
    pub fn idle() -> Self {
        Self {
            chosen: None,
            pressed: None,
            widen: [0.0; STRIP_MAX],
            recording: false,
            dictating: false,
            processing: false,
            agents_only: false,
            failed: false,
            attention: false,
            open: false,
            hover: false,
            reduced: false,
        }
    }
}

pub(crate) struct LookState {
    /// El último cuadro en que algo se movía (`None`: todo quieto).
    last: Option<Instant>,
    /// Por ficha del grupo: cuánto se ensanchó (`Spatial`, con su leve
    /// sobrepaso) y cuánto es la elegida (`Effects`).
    widen: [SpringTrack; STRIP_MAX],
    chosen: [SpringTrack; STRIP_MAX],
    /// Cuánto está apretada `pressed_slot`.
    pub press: Track,
    pub pressed_slot: Option<usize>,
    /// Lo que trabaja fuera de la pill (Reuniones, agentes); lo pone
    /// `Pill::tick`: si está en marcha y si acaba de terminar mal (se borra
    /// al usarlo).
    pub work: (bool, bool),
    /// Lo que trabaja afuera es solo de agentes (no Reuniones); lo pone
    /// `Pill::tick`.
    pub work_agents_only: bool,
    reduced: bool,
    mark: MarkMorph,
}

struct MarkMorph {
    state: MarkState,
    /// Los puntos de donde parte (la forma que se veía al cambiar) y a cuál va.
    from: Vec<(f32, f32)>,
    to: ShapeName,
    shape_t: Track,
    /// Colores de donde parte y rol al que va.
    from_color: Hsla,
    from_ink: Hsla,
    role: Role,
    color_t: Track,
    /// Cuándo se acaba el «hecho».
    done_until: Option<Instant>,
    was_busy: bool,
    /// Desde cuándo procesa y el último cuadro de la secuencia que se pintó.
    loading_since: Option<Instant>,
    loading_seen: Option<u32>,
    /// A cuántos cuadros por segundo recorre la secuencia.
    loading_fps: f32,
    /// Los puntos de la forma de carga del cuadro `loading_seen`: se
    /// recalculan solo cuando cambia de cuadro, no en cada uno de la pill.
    loading_shape: Option<(u32, Vec<(f32, f32)>)>,
}

impl Default for LookState {
    fn default() -> Self {
        let rest = ShapeName::Circle;
        Self {
            last: None,
            widen: std::array::from_fn(|_| SpringTrack::new(0.0, SpringKind::Spatial)),
            chosen: std::array::from_fn(|_| SpringTrack::new(0.0, SpringKind::Effects)),
            press: Track::at(0.0),
            pressed_slot: None,
            work: (false, false),
            work_agents_only: false,
            reduced: false,
            mark: MarkMorph {
                state: MarkState::Rest,
                from: rest.unit_points().to_vec(),
                to: rest,
                shape_t: Track::at(1.0),
                from_color: gpui::transparent_black(),
                from_ink: rgb(0xf0f0ea).into(),
                role: Role::Hidden,
                color_t: Track::at(1.0),
                done_until: None,
                was_busy: false,
                loading_since: None,
                loading_seen: None,
                loading_fps: LOADING_FPS,
                loading_shape: None,
            },
        }
    }
}

/// Los cuadros por segundo de la secuencia de carga: menos si solo procesa un
/// agente.
fn loading_fps(agents_only: bool) -> f32 {
    if agents_only {
        num("pill.m3.loading.agentsFps", LOADING_FPS_AGENTS).max(1.0)
    } else {
        num("pill.m3.loading.fps", LOADING_FPS).max(1.0)
    }
}

/// El cuadro de la secuencia de carga (a `fps`) a `now`.
fn loading_frame(since: Instant, now: Instant, fps: f32) -> u32 {
    (now.saturating_duration_since(since).as_secs_f32() * fps) as u32
}

/// Los puntos de la forma de carga en el cuadro `frame`, en la caja unidad.
/// Como `LoadingIndicator` de gpui-m3: una forma nueva cada 650 ms con un
/// resorte con rebote, un cuarto de vuelta en cada una y encima una vuelta
/// entera cada 4,666 s.
fn loading_points(frame: u32, fps: f32) -> Vec<(f32, f32)> {
    const SEGMENT: f32 = 0.65;
    const SPIN_PERIOD: f32 = 4.666;
    let t = frame as f32 / fps;
    let n = LOADING_SEQUENCE.len();
    let index = (t / SEGMENT) as usize;
    let local = t - index as f32 * SEGMENT;
    let progress = Spring { stiffness: 200.0, damping_ratio: 0.6 }.step(local);
    let morph = Morph {
        from: LOADING_SEQUENCE[index % n],
        to: LOADING_SEQUENCE[(index + 1) % n],
        t: progress,
        rotation: ((index % 4) as f32 + progress) * std::f32::consts::FRAC_PI_2
            + (t / SPIN_PERIOD).fract() * std::f32::consts::TAU,
        toward: None,
    };
    let unit = gpui::Bounds { origin: point(px(0.0), px(0.0)), size: gpui::size(px(1.0), px(1.0)) };
    morph.points(unit).into_iter().map(|p| (f32::from(p.x), f32::from(p.y))).collect()
}

impl LookState {
    /// Avanza un cuadro. Devuelve si algo sigue moviéndose (hay que pedir
    /// otro cuadro). Sin esquema (estilo Atic) no hace nada.
    ///
    /// La cara procesando no cuenta como movimiento: cambia a `LOADING_FPS`
    /// con lo que pide `Pill::tick` (`loading_due`), no a 60.
    pub fn update(&mut self, inputs: LookInputs, scheme: Option<&Scheme>, now: Instant) -> bool {
        let Some(scheme) = scheme else {
            if self.last.is_some() || self.mark.done_until.is_some() || self.mark.state != MarkState::Rest {
                let work = self.work;
                *self = Self::default();
                self.work = work;
            }
            return false;
        };
        // Recién despierta, el primer paso no avanza: sin esto el resorte
        // saltaba de una lo que pasó desde el último movimiento.
        let dt = self.last.map_or(0.0, |last| now.duration_since(last).as_secs_f32().min(0.05));
        self.reduced = inputs.reduced;
        let fps = loading_fps(inputs.agents_only);
        if fps != self.mark.loading_fps {
            self.mark.loading_fps = fps;
            self.mark.loading_shape = None;
        }

        // El grupo: la ficha con el cursor se ensancha y la elegida se
        // rellena, cada una en su lugar (no se desliza de una a otra).
        for index in 0..STRIP_MAX {
            let chosen = if inputs.chosen == Some(index) { 1.0 } else { 0.0 };
            if inputs.reduced {
                self.widen[index].snap(inputs.widen[index]);
                self.chosen[index].snap(chosen);
            } else {
                self.widen[index].set(inputs.widen[index], now);
                self.chosen[index].set(chosen, now);
            }
        }

        match inputs.pressed {
            Some(slot) => {
                if self.pressed_slot != Some(slot) {
                    self.press.snap(0.0);
                }
                self.pressed_slot = Some(slot);
                self.press.set(1.0);
            }
            None => self.press.set(0.0),
        }
        if !self.press.moving() && self.press.value == 0.0 {
            self.pressed_slot = None;
        }

        // La cara.
        let busy = inputs.recording || inputs.dictating || inputs.processing;
        if self.mark.was_busy && !busy && !inputs.failed {
            self.pop_done(now);
        }
        self.mark.was_busy = busy;
        // Que el trabajo de afuera terminó mal vale solo para este final.
        self.work.1 = false;
        if self.mark.done_until.is_some_and(|until| now >= until) {
            self.mark.done_until = None;
        }
        let state = mark_state(
            inputs.recording,
            inputs.dictating,
            inputs.processing,
            self.mark.done_until.is_some(),
            inputs.attention,
            inputs.open,
            inputs.hover,
        );
        if state != self.mark.state {
            let (shape, role) = mark_shape(state);
            let (color, ink) = self.mark_colors(scheme);
            self.mark.from = self.mark_points(now);
            self.mark.from_color = color;
            self.mark.from_ink = ink;
            self.mark.state = state;
            self.mark.to = shape;
            self.mark.role = role;
            self.mark.shape_t.snap(0.0);
            self.mark.shape_t.set(1.0);
            self.mark.color_t.snap(0.0);
            self.mark.color_t.set(1.0);
            self.mark.loading_since = (state == MarkState::Processing).then_some(now);
        }
        self.mark.loading_seen = self.mark.loading_since.map(|since| loading_frame(since, now, fps));
        // La forma de carga de este cuadro de la secuencia, una sola vez.
        match self.mark.loading_seen {
            Some(frame) if !self.reduced => {
                if self.mark.loading_shape.as_ref().is_none_or(|(seen, _)| *seen != frame) {
                    self.mark.loading_shape = Some((frame, loading_points(frame, fps)));
                }
            }
            _ => self.mark.loading_shape = None,
        }

        if inputs.reduced {
            // Sin movimiento: cada cosa salta a su final.
            for track in [&mut self.press, &mut self.mark.shape_t, &mut self.mark.color_t] {
                track.snap(track.target);
            }
        } else {
            self.press.step(Spring::SPATIAL_FAST, dt);
            self.mark.shape_t.step(Spring::SPATIAL_FAST, dt);
            self.mark.color_t.step(Spring::EFFECTS, dt);
        }

        let moving = self.widen.iter().chain(&self.chosen).any(|track| track.is_running(now))
            || self.press.moving()
            || self.mark.shape_t.moving()
            || self.mark.color_t.moving()
            || self.mark.done_until.is_some();
        self.last = moving.then_some(now);
        moving
    }

    /// El «hecho» (Cookie4) por un momento: lo que terminó salió bien.
    pub fn pop_done(&mut self, now: Instant) {
        self.mark.done_until = Some(now + std::time::Duration::from_millis(DONE_MS));
    }

    /// Procesando, si ya toca el cuadro siguiente de la secuencia de carga:
    /// `Pill::tick` redibuja entonces (a `LOADING_FPS`, no a 60). Quieta con
    /// movimiento reducido.
    pub fn loading_due(&self, now: Instant) -> bool {
        !self.reduced
            && self.mark.state == MarkState::Processing
            && self
                .mark
                .loading_since
                .is_some_and(|since| Some(loading_frame(since, now, self.mark.loading_fps)) != self.mark.loading_seen)
    }

    /// Cuánto se ensanchó la ficha `index` a `now` (lo que se ve).
    pub fn widen_of(&self, index: usize, now: Instant) -> f32 {
        self.widen.get(index).map_or(0.0, |track| track.value(now))
    }

    /// Cuánto es la elegida la ficha `index` a `now` (0..1).
    pub fn chosen_of(&self, index: usize, now: Instant) -> f32 {
        self.chosen.get(index).map_or(0.0, |track| track.value(now).clamp(0.0, 1.0))
    }

    /// Los puntos del contenedor de la cara en este momento. Procesando va
    /// hacia la secuencia de carga (o a Circle, quieta, con movimiento
    /// reducido) en vez de a una forma fija.
    fn mark_points(&self, now: Instant) -> Vec<(f32, f32)> {
        let computed;
        let to: &[(f32, f32)] = match (self.mark.state, self.mark.loading_since) {
            (MarkState::Processing, _) if self.reduced => ShapeName::Circle.unit_points(),
            (MarkState::Processing, Some(since)) => {
                let frame = loading_frame(since, now, self.mark.loading_fps);
                match &self.mark.loading_shape {
                    Some((seen, points)) if *seen == frame => points,
                    _ => {
                        computed = loading_points(frame, self.mark.loading_fps);
                        &computed
                    }
                }
            }
            _ => self.mark.to.unit_points(),
        };
        let t = self.mark.shape_t.value;
        self.mark
            .from
            .iter()
            .zip(to)
            .map(|(&(ax, ay), &(bx, by))| (ax + (bx - ax) * t, ay + (by - ay) * t))
            .collect()
    }

    fn mark_colors(&self, scheme: &Scheme) -> (Hsla, Hsla) {
        let (color, ink) = self.mark.role.colors(scheme);
        let t = self.mark.color_t.value;
        (blend(self.mark.from_color, color, t), blend(self.mark.from_ink, ink, t))
    }

    /// El contenedor de la cara para pintar; `None` en reposo del todo.
    pub fn mark_paint(&self, scheme: &Scheme, now: Instant) -> Option<MarkPaint> {
        let (color, ink) = self.mark_colors(scheme);
        if color.a <= 0.01 && self.mark.role == Role::Hidden {
            return None;
        }
        Some(MarkPaint { points: self.mark_points(now), color, ink })
    }
}

// --- Desde la pill ---------------------------------------------------------

impl crate::Pill {
    /// La herramienta cuyo vistazo o panel está abierto, si tiene ficha en la
    /// tira: la ficha elegida del grupo.
    fn chosen_tool(&self) -> Option<usize> {
        if let Some(tool) = self.tool_peek_anchor() {
            return Some(tool);
        }
        if self.peek.target() == 1.0 {
            return Some(crate::CLIPBOARD_TOOL);
        }
        if !self.panel_open {
            return None;
        }
        use crate::NotchTool;
        match self.notch_tool {
            NotchTool::Clipboard => Some(crate::CLIPBOARD_TOOL),
            NotchTool::Textos => Some(crate::TEXTOS_TOOL),
            NotchTool::Agentes => Some(crate::AGENTES_TOOL),
            NotchTool::Sistema => Some(crate::SISTEMA_TOOL),
            NotchTool::Media => Some(crate::MEDIA_TOOL),
            NotchTool::Celular => Some(crate::CELULAR_TOOL),
            NotchTool::Reuniones => Some(crate::REUNIONES_TOOL),
            NotchTool::Pomodoro => Some(crate::POMODORO_TOOL),
            NotchTool::Apps | NotchTool::Personalizar => None,
        }
    }

    /// Avanza lo que anima el estilo Expressive (`LookState`). Devuelve si
    /// hay que pedir otro cuadro. Con el estilo Atic no hace nada.
    pub(crate) fn advance_look(&mut self, now: Instant) -> bool {
        let tokens = PillTokens::current();
        let Some(scheme) = tokens.scheme else {
            return self.look.update(LookInputs::idle(), None, now);
        };
        let slots = self.strip_slots();
        // Cambiando de paso, las fichas no son las mismas: la elegida se apaga.
        let chosen = self
            .chosen_tool()
            .filter(|_| self.page_switch.is_none())
            .and_then(|tool| slots.iter().position(|slot| slot.tool() == Some(tool)));
        let pressed = match &self.press {
            Some(press) if !press.dragging => match press.target {
                crate::PressTarget::Tool(id) => slots.iter().position(|slot| *slot == id),
                _ => None,
            },
            _ => None,
        };
        let mut widen = [0.0; STRIP_MAX];
        for (index, amount) in widen.iter_mut().enumerate().take(slots.len()) {
            *amount = self.strip_widen_target(index);
        }
        let (phase, _) = self.dictation.phase();
        let (work_busy, work_failed) = self.look.work;
        let transcribing = phase == crate::dictation::Phase::Transcribing;
        let inputs = LookInputs {
            chosen,
            pressed,
            widen,
            recording: self.rec_clock.is_some(),
            dictating: matches!(phase, crate::dictation::Phase::Starting | crate::dictation::Phase::Listening),
            processing: transcribing || work_busy,
            agents_only: work_busy && self.look.work_agents_only && !transcribing,
            failed: matches!(phase, crate::dictation::Phase::Error(_)) || work_failed,
            attention: self.update_mark.target() > 0.5,
            open: self.panel_open,
            hover: self.cursor.is_some_and(|c| self.over_face(c, now)),
            reduced: self.reduced_motion,
        };
        self.look.update(inputs, Some(&scheme), now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atic_son_las_constantes_de_siempre() {
        let t = PillTokens::atic(Skin::Atic);
        assert_eq!(t.skin, Skin::Atic.mid());
        assert_eq!(t.fill, Skin::Atic);
        assert_eq!(t.face, Hsla::from(rgb(0xf0f0ea)));
        assert_eq!(t.on, Hsla::from(rgb(0xf0f0ea)));
        assert_eq!(t.off, Hsla::from(rgb(0xa8a89e)));
        assert_eq!(t.hover_grow, 0.14);
        assert_eq!(t.icon, crate::TOOL_ICON);
        assert!(t.layer.is_none() && t.segment.is_none() && t.chosen.is_none() && t.recording.is_none() && t.tooltip.is_none());
        assert_eq!(t.segment_color(1.0, 1.0, 1.0), None);
        assert_eq!(t.shadow(1.0), vec![crate::pill_shadow(1.0)]);
        assert_eq!(t.shadow(0.4), vec![crate::pill_shadow(0.4)]);
        // Con otra piel solo cambia la piel.
        let wine = Skin::Solid { color: 0x3b1220 };
        assert_eq!(PillTokens::atic(wine).skin, wine.mid());
        assert_eq!(PillTokens::atic(wine).fill, wine);
    }

    #[test]
    fn expressive_sale_del_esquema_oscuro() {
        let t = PillTokens::expressive(Skin::Atic);
        let s = Scheme::baseline_dark();
        assert_eq!(t.skin, s.surface_container);
        assert_eq!(t.fill, Skin::Solid { color: to_hex(s.surface_container) });
        assert_eq!(t.chosen, Some((s.secondary_container, s.on_secondary_container)));
        assert_eq!(t.segment, Some(s.surface_container_high));
        assert_eq!(t.segment_color(0.0, 0.0, 0.0).map(to_hex), Some(to_hex(s.surface_container_high)));
        assert_eq!(t.segment_color(0.0, 0.0, 1.0).map(to_hex), Some(to_hex(s.secondary_container)));
        assert_eq!(t.hover_grow, 0.0);
        assert_eq!(t.shadow(1.0).len(), 2);
    }

    #[test]
    fn las_pieles_de_la_lista_no_llevan_velo() {
        for skin in [
            Skin::Atic,
            Skin::Solid { color: 0x000000 },
            Skin::Solid { color: 0x3b1220 },
            Skin::Gradient { from: 0x2b1055, to: 0x7d2a5c, via: None, angle: 90 },
        ] {
            assert!(PillTokens::atic(skin).veil.is_none(), "{skin:?}");
            assert!(PillTokens::expressive(skin).veil.is_none(), "{skin:?}");
        }
    }

    #[test]
    fn una_piel_clara_lleva_velo_hasta_que_se_lee() {
        let white = Skin::Solid { color: 0xffffff };
        let atic = PillTokens::atic(white);
        let veil = atic.veil.expect("velo");
        let under = blend(rgb(0xffffff).into(), Hsla { a: 1.0, ..veil }, veil.a);
        assert!(contrast_ratio(rgb(0xf0f0ea).into(), under) >= TEXT_CONTRAST);
        let m3 = PillTokens::expressive(Skin::Gradient { from: 0x101010, to: 0xfff3c4, via: None, angle: 90 });
        let veil = m3.veil.expect("velo");
        let s = m3.scheme.unwrap();
        let under = blend(rgb(0xfff3c4).into(), Hsla { a: 1.0, ..veil }, veil.a);
        assert!(contrast_ratio(s.on_surface, under) >= TEXT_CONTRAST);
        assert!(contrast_ratio(s.on_surface_variant, under) >= TEXT_CONTRAST);
    }

    #[test]
    fn expressive_pinta_la_piel_o_solo_la_siembra() {
        let dusk = Skin::Gradient { from: 0x2b1055, to: 0x7d2a5c, via: Some(0x4a1a60), angle: 135 };
        let painted = PillTokens::expressive_with(SchemeKey::plain(dusk), true);
        assert_eq!(painted.fill, dusk);
        assert_eq!(painted.segment.map(|s| s.a), Some(SEGMENT_ON_SKIN));
        let seeded = PillTokens::expressive_with(SchemeKey::plain(dusk), false);
        let s = seeded.scheme.unwrap();
        assert_eq!(seeded.fill, Skin::Solid { color: to_hex(s.surface_container) });
        assert_eq!(seeded.segment, Some(s.surface_container_high));
    }

    #[test]
    fn la_clave_del_esquema_distingue_claro_y_oscuro() {
        let skin = Skin::Solid { color: 0x3b1220 };
        let light = SchemeKey { dark: false, ..SchemeKey::plain(skin) };
        assert_ne!(SchemeKey::plain(skin), light);
        let (dark, light) = (scheme_for(SchemeKey::plain(skin)), scheme_for(light));
        assert_ne!(dark, light);
        assert!(luminance(light.surface) > 0.5 && luminance(dark.surface) < 0.2);
        assert!(luminance(light.on_surface) < 0.2);
        // La de Atic de fábrica: el par de siempre de gpui-m3.
        assert_eq!(scheme_for(SchemeKey { dark: false, ..SchemeKey::plain(Skin::Atic) }), Scheme::baseline_light());
        // Los tokens del tema claro salen de su esquema.
        let t = PillTokens::expressive_with(SchemeKey { dark: false, ..SchemeKey::plain(Skin::Atic) }, true);
        let s = Scheme::baseline_light();
        assert_eq!((t.skin, t.on, t.off), (s.surface_container, s.on_surface, s.on_surface_variant));
    }

    #[test]
    fn con_tema_claro_la_piel_clara_no_lleva_velo_y_la_oscura_si() {
        let cream = Skin::Solid { color: 0xfff3c4 };
        let light = |skin| PillTokens::expressive_with(SchemeKey { dark: false, ..SchemeKey::plain(skin) }, true);
        assert!(light(cream).veil.is_none());
        assert!(light(Skin::Solid { color: 0xffffff }).veil.is_none());
        // Con tema oscuro esa misma piel sí lo lleva.
        assert!(PillTokens::expressive(cream).veil.is_some());
        // Una piel oscura con tema claro se aclara hasta que el texto oscuro se lea.
        let night = light(Skin::Solid { color: 0x14213d });
        let veil = night.veil.expect("velo");
        let s = night.scheme.unwrap();
        let under = blend(rgb(0x14213d).into(), Hsla { a: 1.0, ..veil }, veil.a);
        assert!(contrast_ratio(s.on_surface, under) >= TEXT_CONTRAST);
        // El texto secundario también: 4,5:1, no el 3:1 de los íconos.
        assert!(contrast_ratio(s.on_surface_variant, under) >= TEXT_CONTRAST);
    }

    #[test]
    fn fundida_con_el_notch_o_sobre_la_caratula_la_pill_va_oscura() {
        let look = Appearance { pill: PillStyle::Expressive, pill_theme: crate::appearance::PillTheme::Light, ..Appearance::default() };
        assert!(!SchemeKey::of(&look).dark);
        set_force_dark(true);
        let fused = SchemeKey::of_pill(&look);
        set_force_dark(false);
        assert!(fused.dark);
        // La muestra de Ajustes no mira el notch.
        assert!(!SchemeKey::of(&look).dark);
    }

    #[test]
    fn los_colores_fijos_toman_su_rol_y_los_acentos_no() {
        let light = Scheme::baseline_light();
        assert_eq!(ink_in(0xf0f0ea, &light), light.on_surface);
        assert_eq!(ink_in(0xa8a89e, &light), light.on_surface_variant);
        assert_eq!(ink_in(0x1a1a18, &light), light.surface);
        // El texto sobre su tinta se sigue leyendo en el tema claro.
        assert!(contrast_ratio(ink_in(0x1a1a18, &light), ink_in(0xf0f0ea, &light)) >= TEXT_CONTRAST);
        assert_eq!(ink_in(0xff5a52, &light), Hsla::from(rgb(0xff5a52)));
    }

    #[test]
    fn el_esquema_cambia_con_el_estilo_de_color_y_el_contraste() {
        let skin = Skin::Solid { color: 0x3b1220 };
        let plain = scheme_for(SchemeKey::plain(skin));
        let vibrant = scheme_for(SchemeKey { style: SchemeStyle::Vibrant, ..SchemeKey::plain(skin) });
        let high = scheme_for(SchemeKey { contrast: 1.0, ..SchemeKey::plain(skin) });
        assert_ne!(plain, vibrant);
        assert_ne!(plain, high);
        // Con más contraste, el texto se separa más de la superficie.
        assert!(contrast_ratio(high.on_surface_variant, high.surface) > contrast_ratio(plain.on_surface_variant, plain.surface));
        // La de Atic de fábrica sigue siendo `baseline_dark`; con otro estilo, ya no.
        assert_eq!(scheme_for(SchemeKey::plain(Skin::Atic)), Scheme::baseline_dark());
        assert_ne!(scheme_for(SchemeKey { style: SchemeStyle::Vibrant, ..SchemeKey::plain(Skin::Atic) }), Scheme::baseline_dark());
        // Un acento manda sobre la piel.
        let accent = scheme_for(SchemeKey { accent: Some(0x0061a4), ..SchemeKey::plain(skin) });
        assert_eq!(accent, scheme_for(SchemeKey { accent: Some(0x0061a4), ..SchemeKey::plain(Skin::Atic) }));
    }

    #[test]
    fn la_cara_dice_lo_mas_importante() {
        assert_eq!(mark_state(true, true, true, true, true, true, true), MarkState::Recording);
        assert_eq!(mark_state(false, true, true, false, true, true, true), MarkState::Dictating);
        assert_eq!(mark_state(false, false, true, true, true, true, true), MarkState::Processing);
        assert_eq!(mark_state(false, false, false, true, true, true, true), MarkState::Done);
        assert_eq!(mark_state(false, false, false, false, true, true, true), MarkState::Attention);
        assert_eq!(mark_state(false, false, false, false, false, true, true), MarkState::Open);
        assert_eq!(mark_state(false, false, false, false, false, false, true), MarkState::Hover);
        assert_eq!(mark_state(false, false, false, false, false, false, false), MarkState::Rest);
    }

    #[test]
    fn cada_estado_tiene_su_forma_y_su_rol() {
        assert_eq!(mark_shape(MarkState::Rest), (ShapeName::Circle, Role::Hidden));
        assert_eq!(mark_shape(MarkState::Hover), (ShapeName::Cookie9, Role::Highest));
        assert_eq!(mark_shape(MarkState::Open), (ShapeName::Square, Role::Secondary));
        assert_eq!(mark_shape(MarkState::Dictating), (ShapeName::Sunny, Role::Primary));
        assert_eq!(mark_shape(MarkState::Recording), (ShapeName::SoftBurst, Role::Error));
        assert_eq!(mark_shape(MarkState::Attention), (ShapeName::Clover4, Role::Tertiary));
        assert_eq!(mark_shape(MarkState::Done), (ShapeName::Cookie4, Role::Primary));
        assert_eq!(mark_shape(MarkState::Processing), (LOADING_SEQUENCE[0], Role::Secondary));
    }

    fn inputs() -> LookInputs {
        LookInputs::idle()
    }

    #[test]
    fn en_reposo_no_pide_cuadros() {
        let s = Scheme::baseline_dark();
        let mut look = LookState::default();
        assert!(!look.update(inputs(), Some(&s), Instant::now()));
        // En Atic nunca.
        assert!(!look.update(LookInputs { hover: true, ..inputs() }, None, Instant::now()));
    }

    #[test]
    fn la_elegida_se_rellena_en_su_lugar_y_pasa_a_otra() {
        let s = Scheme::baseline_dark();
        let mut look = LookState::default();
        let mut now = Instant::now();
        assert!(look.update(LookInputs { chosen: Some(3), ..inputs() }, Some(&s), now));
        for _ in 0..120 {
            now += std::time::Duration::from_millis(16);
            look.update(LookInputs { chosen: Some(1), ..inputs() }, Some(&s), now);
        }
        assert!((look.chosen_of(1, now) - 1.0).abs() < 1e-3);
        assert!(look.chosen_of(3, now) < 1e-3);
        // Las de en medio nunca se encendieron: no hay píldora que pase por ellas.
        assert_eq!(look.chosen_of(2, now), 0.0);
    }

    #[test]
    fn la_ficha_con_el_cursor_se_ensancha_con_resorte_y_vuelve() {
        let s = Scheme::baseline_dark();
        let mut look = LookState::default();
        let mut now = Instant::now();
        let mut widen = [0.0; STRIP_MAX];
        widen[2] = 1.0;
        let mut peak: f32 = 0.0;
        assert!(look.update(LookInputs { widen, ..inputs() }, Some(&s), now));
        for _ in 0..120 {
            now += std::time::Duration::from_millis(16);
            look.update(LookInputs { widen, ..inputs() }, Some(&s), now);
            peak = peak.max(look.widen_of(2, now));
        }
        assert_eq!(look.widen_of(2, now), 1.0);
        // `Spatial` se pasa un poco antes de asentarse: se lee como resorte.
        assert!(peak > 1.0);
        assert_eq!(look.widen_of(1, now), 0.0);
        for _ in 0..120 {
            now += std::time::Duration::from_millis(16);
            look.update(inputs(), Some(&s), now);
        }
        assert_eq!(look.widen_of(2, now), 0.0);
        assert!(!look.update(inputs(), Some(&s), now));
    }

    #[test]
    fn con_movimiento_reducido_salta_al_final() {
        let s = Scheme::baseline_dark();
        let mut look = LookState::default();
        let now = Instant::now();
        let moving = look.update(LookInputs { chosen: Some(2), hover: true, reduced: true, ..inputs() }, Some(&s), now);
        assert!(!moving);
        assert_eq!(look.chosen_of(2, now), 1.0);
        let paint = look.mark_paint(&s, now).unwrap();
        assert_eq!(to_hex(paint.color), to_hex(s.surface_container_highest));
        for (a, b) in paint.points.iter().zip(ShapeName::Cookie9.unit_points()) {
            assert!((a.0 - b.0).abs() < 1e-4 && (a.1 - b.1).abs() < 1e-4);
        }
    }

    #[test]
    fn al_terminar_de_grabar_hace_pop_y_vuelve() {
        let s = Scheme::baseline_dark();
        let mut look = LookState::default();
        let mut now = Instant::now();
        look.update(LookInputs { recording: true, reduced: true, ..inputs() }, Some(&s), now);
        now += std::time::Duration::from_millis(16);
        assert!(look.update(LookInputs { reduced: true, ..inputs() }, Some(&s), now));
        assert_eq!(look.mark.state, MarkState::Done);
        now += std::time::Duration::from_millis(DONE_MS + 10);
        assert!(!look.update(LookInputs { reduced: true, ..inputs() }, Some(&s), now));
        assert_eq!(look.mark.state, MarkState::Rest);
    }

    fn close(a: &[(f32, f32)], b: &[(f32, f32)]) -> bool {
        a.len() == b.len() && a.iter().zip(b).all(|(a, b)| (a.0 - b.0).abs() < 1e-4 && (a.1 - b.1).abs() < 1e-4)
    }

    #[test]
    fn procesando_recorre_la_secuencia_solo_a_15_fps_y_sin_pedir_cuadros() {
        let s = Scheme::baseline_dark();
        let mut look = LookState::default();
        let start = Instant::now();
        let mut now = start;
        // Al empezar se mueve (la forma llega); después, quieta entre cuadros.
        for _ in 0..60 {
            look.update(LookInputs { processing: true, ..inputs() }, Some(&s), now);
            now += std::time::Duration::from_millis(16);
        }
        assert_eq!(look.mark.state, MarkState::Processing);
        assert!(!look.update(LookInputs { processing: true, ..inputs() }, Some(&s), now));
        // Nada nuevo dentro del mismo cuadro de 1/15 s; el siguiente sí.
        assert!(!look.loading_due(now));
        let frame = loading_frame(start, now, LOADING_FPS);
        let next = start + std::time::Duration::from_secs_f32((frame + 1) as f32 / LOADING_FPS + 1e-3);
        assert!(look.loading_due(next));
        let a = look.mark_paint(&s, now).unwrap().points;
        let b = look.mark_paint(&s, next + std::time::Duration::from_millis(300)).unwrap().points;
        assert!(!close(&a, &b), "la forma cambia mientras procesa");
        // Al terminar bien, «hecho»; después nada pide cuadros.
        assert!(look.update(inputs(), Some(&s), next));
        assert_eq!(look.mark.state, MarkState::Done);
        assert!(!look.loading_due(next + std::time::Duration::from_secs(1)));
    }

    #[test]
    fn procesando_con_movimiento_reducido_queda_en_circulo() {
        let s = Scheme::baseline_dark();
        let mut look = LookState::default();
        let now = Instant::now();
        assert!(!look.update(LookInputs { processing: true, reduced: true, ..inputs() }, Some(&s), now));
        let later = now + std::time::Duration::from_secs(2);
        assert!(!look.loading_due(later));
        let paint = look.mark_paint(&s, later).unwrap();
        assert!(close(&paint.points, ShapeName::Circle.unit_points()));
    }

    #[test]
    fn el_trabajo_de_afuera_que_falla_no_hace_pop() {
        let s = Scheme::baseline_dark();
        let mut look = LookState::default();
        let mut now = Instant::now();
        look.update(LookInputs { processing: true, reduced: true, ..inputs() }, Some(&s), now);
        now += std::time::Duration::from_millis(16);
        look.update(LookInputs { failed: true, reduced: true, ..inputs() }, Some(&s), now);
        assert_eq!(look.mark.state, MarkState::Rest);
    }

    #[test]
    fn guardar_una_captura_hace_pop() {
        let s = Scheme::baseline_dark();
        let mut look = LookState::default();
        let now = Instant::now();
        look.pop_done(now);
        assert!(look.update(LookInputs { reduced: true, ..inputs() }, Some(&s), now));
        assert_eq!(look.mark.state, MarkState::Done);
        // En Atic se olvida sin pedir cuadros.
        let mut atic = LookState::default();
        atic.pop_done(now);
        assert!(!atic.update(inputs(), None, now));
        assert!(atic.mark.done_until.is_none());
    }

    // --- El grupo de la tira ---

    const W: f32 = 44.0;
    const GAP: f32 = 2.0;

    fn total(layout: &[(f32, f32)]) -> f32 {
        layout.last().map_or(0.0, |&(start, width)| start + width)
    }

    #[test]
    fn el_grupo_no_cambia_de_largo_con_el_hover() {
        for count in 1..=STRIP_MAX {
            let rest = group_layout(count, W, GAP, 12.0, &[]);
            assert!((total(&rest) - (count as f32 * (W + GAP) - GAP)).abs() < 1e-3);
            for hovered in 0..count {
                for amount in [0.3, 1.0, 1.2] {
                    let mut widen = vec![0.0; count];
                    widen[hovered] = amount;
                    let layout = group_layout(count, W, GAP, 12.0, &widen);
                    assert!((total(&layout) - total(&rest)).abs() < 1e-3, "count {count} hovered {hovered}");
                }
                // Dos a la vez (cursor en una, vistazo de otra): tampoco.
                let mut widen = vec![0.0; count];
                widen[hovered] = 1.0;
                widen[(hovered + 1) % count] = 0.5;
                assert!((total(&group_layout(count, W, GAP, 12.0, &widen)) - total(&rest)).abs() < 1e-3);
            }
        }
    }

    #[test]
    fn la_ficha_con_hover_es_mas_ancha_y_sus_vecinas_ceden() {
        let mut widen = vec![0.0; 5];
        widen[2] = 1.0;
        let layout = group_layout(5, W, GAP, 12.0, &widen);
        assert_eq!(layout[2].1, W + 12.0);
        assert_eq!(layout[1].1, W - 6.0);
        assert_eq!(layout[3].1, W - 6.0);
        assert_eq!(layout[0].1, W);
        // Al medio, su centro no se mueve.
        let rest = group_layout(5, W, GAP, 12.0, &[]);
        assert!((layout[2].0 + layout[2].1 / 2.0 - (rest[2].0 + rest[2].1 / 2.0)).abs() < 1e-3);
        // En un extremo cede todo la única vecina.
        widen = vec![0.0; 5];
        widen[0] = 1.0;
        let layout = group_layout(5, W, GAP, 12.0, &widen);
        assert_eq!((layout[0].1, layout[1].1), (W + 12.0, W - 12.0));
        // Sola no tiene a quién quitarle: no crece.
        assert_eq!(group_layout(1, W, GAP, 12.0, &[1.0]), vec![(0.0, W)]);
        // En Atic (sin crecer) es la tira de siempre.
        let atic = group_layout(4, W, GAP, 0.0, &[1.0, 0.0, 1.0, 0.0]);
        for (index, &(start, width)) in atic.iter().enumerate() {
            assert_eq!((start, width), (index as f32 * (W + GAP), W));
        }
    }

    #[test]
    fn el_hit_test_no_deja_huecos_ni_alterna_en_el_borde() {
        let count = 6;
        let rest = group_layout(count, W, GAP, 12.0, &[]);
        // Sin hover, la cuenta de siempre: centro ± (ancho + espacio) / 2.
        for along in [0.0, 21.9, 45.0, 46.0, 90.0, total(&rest)] {
            let old = (0..count).find(|&i| (along - (i as f32 * (W + GAP) + W / 2.0)).abs() <= (W + GAP) / 2.0);
            assert_eq!(group_slot_at(&rest, GAP, 1.0, along), old, "en {along}");
        }
        // Barrer todo el grupo: siempre hay ficha (sin huecos muertos)…
        let mut along = -GAP / 2.0;
        while along <= total(&rest) + GAP / 2.0 {
            assert!(group_slot_at(&rest, GAP, 1.0, along).is_some(), "hueco en {along}");
            along += 0.25;
        }
        // …y el hover es estable: la ficha bajo el cursor, ya ensanchada (la
        // geometría objetivo), sigue bajo el cursor. Si no, el borde se
        // correría y el hover saltaría entre dos fichas en cada cuadro.
        let mut along = 0.0;
        while along <= total(&rest) {
            let first = group_slot_at(&rest, GAP, 1.0, along).unwrap();
            let mut widen = vec![0.0; count];
            widen[first] = 1.0;
            let grown = group_layout(count, W, GAP, 12.0, &widen);
            assert_eq!(group_slot_at(&grown, GAP, 1.0, along), Some(first), "en {along}");
            along += 0.25;
        }
    }

    #[test]
    fn las_esquinas_del_grupo() {
        let thick = 32.0;
        // Extremos redondos, interiores chicas.
        assert_eq!(group_radii(0, 4, thick, 0.0, 0.0), (16.0, GROUP_INNER));
        assert_eq!(group_radii(1, 4, thick, 0.0, 0.0), (GROUP_INNER, GROUP_INNER));
        assert_eq!(group_radii(3, 4, thick, 0.0, 0.0), (GROUP_INNER, 16.0));
        assert_eq!(group_radii(0, 1, thick, 0.0, 0.0), (16.0, 16.0));
        // La elegida, redonda entera; apretada, más cuadrada.
        assert_eq!(group_radii(1, 4, thick, 1.0, 0.0), (16.0, 16.0));
        assert_eq!(group_radii(1, 4, thick, 0.0, 1.0), (GROUP_PRESSED, GROUP_PRESSED));
        assert_eq!(group_radii(0, 4, thick, 1.0, 1.0), (GROUP_PRESSED, GROUP_PRESSED));
    }

    // --- Cachés ---

    #[test]
    fn el_cache_de_esquemas_bota_el_menos_usado() {
        let mut entries: Vec<(u32, u32)> = Vec::new();
        for key in 0..3 {
            lru_put(&mut entries, key, key * 10, 3);
        }
        // Usar el más antiguo lo deja al final…
        assert_eq!(lru_get(&mut entries, &0), Some(0));
        assert_eq!(entries.iter().map(|(key, _)| *key).collect::<Vec<_>>(), vec![1, 2, 0]);
        // …y al llenarse se va el que lleva más sin usarse, no el que se usa.
        lru_put(&mut entries, 3, 30, 3);
        assert_eq!(entries.iter().map(|(key, _)| *key).collect::<Vec<_>>(), vec![2, 0, 3]);
        assert_eq!(lru_get(&mut entries, &1), None);
        assert_eq!(lru_get(&mut entries, &0), Some(0));
        assert_eq!(lru_get(&mut entries, &3), Some(30));
        assert_eq!(entries.len(), 3);
    }

    #[test]
    fn los_tokens_se_arman_una_vez_por_clave() {
        static SLOT: Mutex<Option<((PillStyle, SchemeKey, bool), PillTokens)>> = Mutex::new(None);
        let made = std::cell::Cell::new(0);
        let skin = Skin::Solid { color: 0x3b1220 };
        let key = (PillStyle::Expressive, SchemeKey::plain(skin), true);
        let make = || {
            made.set(made.get() + 1);
            PillTokens::expressive_with(SchemeKey::plain(skin), true)
        };
        let first = remember(&SLOT, key, make);
        let again = remember(&SLOT, key, make);
        assert_eq!(made.get(), 1);
        assert_eq!(first.skin, again.skin);
        assert_eq!(first.shadow(0.5), again.shadow(0.5));
        // Otra piel, otro esquema o sin pintar la piel: se arman de nuevo.
        remember(&SLOT, (PillStyle::Expressive, SchemeKey::plain(Skin::Atic), true), make);
        remember(&SLOT, (PillStyle::Expressive, SchemeKey::plain(Skin::Atic), false), make);
        remember(&SLOT, (PillStyle::Atic, SchemeKey::plain(Skin::Atic), false), make);
        assert_eq!(made.get(), 4);
    }

    #[test]
    fn procesando_solo_por_agentes_va_a_10_fps() {
        let s = Scheme::baseline_dark();
        let mut look = LookState::default();
        let start = Instant::now();
        let agents = LookInputs { processing: true, agents_only: true, ..inputs() };
        look.update(agents, Some(&s), start);
        // 1/15 s después todavía es el mismo cuadro; a 1/10 s, el siguiente.
        assert!(!look.loading_due(start + std::time::Duration::from_millis(70)));
        assert!(look.loading_due(start + std::time::Duration::from_millis(101)));
        // Con dictado o Reuniones, 15.
        let mut look = LookState::default();
        look.update(LookInputs { processing: true, ..inputs() }, Some(&s), start);
        assert!(look.loading_due(start + std::time::Duration::from_millis(70)));
    }

    #[test]
    fn la_forma_de_carga_guardada_es_la_del_cuadro() {
        let s = Scheme::baseline_dark();
        let mut look = LookState::default();
        let start = Instant::now();
        let mut now = start;
        for _ in 0..60 {
            look.update(LookInputs { processing: true, ..inputs() }, Some(&s), now);
            now += std::time::Duration::from_millis(16);
        }
        let frame = loading_frame(start, now, LOADING_FPS);
        look.update(LookInputs { processing: true, ..inputs() }, Some(&s), now);
        let (seen, points) = look.mark.loading_shape.clone().unwrap();
        assert_eq!(seen, frame);
        assert!(close(&points, &loading_points(frame, LOADING_FPS)));
        // Ya asentada, lo que se pinta es esa forma.
        assert!(close(&look.mark_paint(&s, now).unwrap().points, &points));
    }
}

/// El esquema del contenido de los vistazos y de los paneles del notch: el de
/// la pill en Expressive (sembrado con la piel, claro u oscuro según el tema
/// de la pill); `None` en Atic, donde cada panel sigue con sus colores de
/// siempre (oscuros).
pub(crate) fn content_scheme() -> Option<Scheme> {
    content_key().map(scheme_for)
}

/// La clave del esquema del contenido; `None` en Atic.
fn content_key() -> Option<SchemeKey> {
    let look = crate::appearance::current();
    (look.pill == PillStyle::Expressive).then(|| SchemeKey::of_pill(&look))
}

/// ¿Va claro el contenido de la pill? Solo en Expressive con tema claro (y
/// sin estar fundida con el notch).
pub(crate) fn content_light() -> bool {
    content_key().is_some_and(|key| !key.dark)
}

/// El esquema de lo que va sobre una carátula o una imagen (Ahora suena):
/// siempre oscuro, con su texto claro y el degradado oscuro de debajo, sea
/// cual sea el tema. `None` en Atic.
pub(crate) fn cover_scheme() -> Option<Scheme> {
    content_key().map(|key| scheme_for(SchemeKey { dark: true, ..key }))
}

/// Un color fijo del contenido de la pill de siempre (oscura) en el estilo
/// vigente: en Atic, tal cual; en Expressive, el rol del esquema que hace su
/// papel (`role_of`), así se lee también en el tema claro.
pub(crate) fn ink(color: u32) -> Hsla {
    match content_scheme() {
        Some(s) => ink_in(color, &s),
        None => rgb(color).into(),
    }
}

/// `ink` con el esquema ya en la mano.
pub(crate) fn ink_in(color: u32, s: &Scheme) -> Hsla {
    role_of(color, s).unwrap_or_else(|| rgb(color).into())
}

/// El rol que hace el papel de un color fijo del contenido oscuro de
/// siempre: el texto, sus grises, la tinta oscura sobre una ficha clara y los
/// fondos de burbuja. Uno que no es de la lista (un acento, el rojo de
/// grabar) no tiene: sale igual en los dos temas.
fn role_of(color: u32, s: &Scheme) -> Option<Hsla> {
    Some(match color {
        0xffffff | 0xf0f0ea | 0xe8e8e0 | 0xc9c9c0 => s.on_surface,
        0xb8b8ae | 0xa8a89e | 0x9a9a90 | 0x8f8f86 | 0x8a8a82 => s.on_surface_variant,
        0x6e6e66 => s.outline,
        0x1a1a18 => s.surface,
        0x2a2a28 | 0x2a2a27 => s.surface_container_high,
        _ => return None,
    })
}

/// En Expressive, mientras se dibuja la ventana de la pill, los componentes de
/// gpui-m3 (vistazos, paneles del notch) usan el esquema de la pill y no el de
/// Atic Code. Devuelve la capa que, pintada al final, deja el de antes, como
/// `theme::scope_m3`. En Atic no hace nada.
pub(crate) fn scope_m3(cx: &mut gpui::App) -> Option<gpui::AnyElement> {
    content_key().map(|key| gpui_m3::Theme::scoped(scheme_for(key), key.dark, cx))
}

impl Track {
    /// Avanza el resorte `dt` segundos, para lo que se anima fuera de
    /// `LookState` (la rueda de `wheel_m3.rs`).
    pub(crate) fn advance(&mut self, spring: Spring, dt: f32) {
        self.step(spring, dt);
    }
}
