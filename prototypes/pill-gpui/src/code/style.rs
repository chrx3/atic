//! Los estilos de Atic Code, los mismos de la referencia (`src/styles/tokens.css`):
//! Formal, Material 3 Expressive y Liquid Glass, cada uno en claro y oscuro.
//! La vista solo usa estos nombres; cada estilo pone valores, forma y si los
//! paneles flotan.
//!
//! El modo «Sistema» sigue al claro/oscuro de Windows (también en vivo, con la
//! apariencia de la ventana) salvo que se fije claro u oscuro. Cada espacio
//! puede tener su color de acento: en M3 rehace el esquema desde esa semilla;
//! en Formal y Glass cambia el acento con el tono ajustado al modo, como
//! `simpleAccent` de la referencia. Liquid Glass hace la ventana translúcida sobre el escritorio
//! desenfocado (Acrylic de Windows); GPUI no desenfoca lo que queda detrás de
//! un panel dentro de la ventana, así que los paneles son vidrio sobre ese fondo.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU8, Ordering};

use gpui::{Hsla, Rgba};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Style {
    Formal,
    #[default]
    Expressive,
    Glass,
}

impl Style {
    pub const ALL: [(Style, &'static str); 3] =
        [(Style::Formal, "Formal"), (Style::Expressive, "Material 3 Expressive"), (Style::Glass, "Liquid Glass")];

    /// Su nombre en `code-claude.json` (el acento de cada estilo).
    pub fn key(self) -> &'static str {
        match self {
            Style::Formal => "formal",
            Style::Expressive => "expressive",
            Style::Glass => "glass",
        }
    }

    /// Los colores sugeridos de la referencia (`ACCENT_PRESETS`): el primero es el del estilo.
    pub fn accent_presets(self) -> [u32; 9] {
        match self {
            Style::Formal => [0x2563eb, 0x7c3aed, 0xdb2777, 0xdc2626, 0xea580c, 0xca8a04, 0x16a34a, 0x0891b2, 0x475569],
            Style::Expressive => [0x6750a4, 0x0b57d0, 0x006a6a, 0x386a20, 0x7d5700, 0xb3261e, 0x984061, 0x5b5f97, 0x00639b],
            Style::Glass => [0x007aff, 0x5856d6, 0xaf52de, 0xff2d55, 0xff3b30, 0xff9500, 0xffcc00, 0x34c759, 0x30b0c7],
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// El de Windows.
    #[default]
    System,
    Light,
    Dark,
}

impl Mode {
    pub const ALL: [(Mode, &'static str); 3] = [(Mode::System, "Sistema"), (Mode::Light, "Claro"), (Mode::Dark, "Oscuro")];
}

#[derive(Clone, Copy)]
pub struct Tokens {
    pub style: Style,
    pub light: bool,
    /// El fondo de la ventana.
    pub bg: Hsla,
    /// Los paneles de los costados.
    pub pane: Hsla,
    /// El chat.
    pub editor: Hsla,
    /// Menús y tarjetas.
    pub raised: Hsla,
    pub control: Hsla,
    pub control2: Hsla,
    pub border: Hsla,
    pub text: Hsla,
    pub muted: Hsla,
    pub faint: Hsla,
    pub accent: Hsla,
    pub on_accent: Hsla,
    pub accent_soft: Hsla,
    pub on_accent_soft: Hsla,
    /// Lo que pide atención (un permiso): el terciario de M3.
    pub attention: Hsla,
    pub on_attention: Hsla,
    pub hover: Hsla,
    pub sel: Hsla,
    pub ok: Hsla,
    pub bad: Hsla,
    pub warn: Hsla,
    pub add: Hsla,
    pub del: Hsla,
    /// El borde de luz de arriba del vidrio.
    pub highlight: Hsla,
    pub shadow: Hsla,
    pub r_pane: f32,
    pub r_ctl: f32,
    pub r_btn: f32,
    pub r_pop: f32,
    pub r_chip: f32,
    /// Separación entre paneles flotantes (0: pegados, con bordes).
    pub gap: f32,
    pub font: &'static str,
    pub mono: &'static str,
    pub fs: f32,
}

fn hex(color: u32) -> Hsla {
    Rgba { r: (color >> 16 & 0xff) as f32 / 255.0, g: (color >> 8 & 0xff) as f32 / 255.0, b: (color & 0xff) as f32 / 255.0, a: 1.0 }
        .into()
}

fn rgba(r: u8, g: u8, b: u8, a: f32) -> Hsla {
    Rgba { r: r as f32 / 255.0, g: g as f32 / 255.0, b: b as f32 / 255.0, a }.into()
}

const UI_FONT: &str = "Segoe UI Variable Text";
const MONO_FONT: &str = "Cascadia Mono";

pub fn tokens(style: Style, light: bool) -> Tokens {
    match (style, light) {
        (Style::Formal, false) => Tokens {
            style,
            light,
            bg: hex(0x0e0f11),
            pane: hex(0x131417),
            editor: hex(0x0e0f11),
            raised: hex(0x1a1c20),
            control: hex(0x1d1f24),
            control2: hex(0x24262b),
            border: hex(0x24262b),
            text: hex(0xe6e7ea),
            muted: hex(0x8b8f98),
            faint: hex(0x5c6069),
            accent: hex(0x4c8dff),
            on_accent: hex(0xffffff),
            accent_soft: rgba(76, 141, 255, 0.14),
            on_accent_soft: hex(0x8ab4ff),
            attention: hex(0x1a1c20),
            on_attention: hex(0xe6e7ea),
            hover: hex(0x1d1f24),
            sel: hex(0x1c2638),
            ok: hex(0x3fb27f),
            bad: hex(0xf06a62),
            warn: hex(0xd9ae3a),
            add: rgba(63, 178, 127, 0.13),
            del: rgba(240, 106, 98, 0.13),
            highlight: rgba(255, 255, 255, 0.0),
            shadow: rgba(0, 0, 0, 0.55),
            r_pane: 0.,
            r_ctl: 6.,
            r_btn: 6.,
            r_pop: 10.,
            r_chip: 6.,
            gap: 0.,
            font: UI_FONT,
            mono: MONO_FONT,
            fs: 13.5,
        },
        (Style::Formal, true) => Tokens {
            style,
            light,
            bg: hex(0xffffff),
            pane: hex(0xf7f7f8),
            editor: hex(0xffffff),
            raised: hex(0xffffff),
            control: hex(0xeeeff1),
            control2: hex(0xe4e5e8),
            border: hex(0xe4e5e8),
            text: hex(0x17181b),
            muted: hex(0x5f636b),
            faint: hex(0xa0a4ac),
            accent: hex(0x2563eb),
            on_accent: hex(0xffffff),
            accent_soft: rgba(37, 99, 235, 0.09),
            on_accent_soft: hex(0x0b5bd3),
            attention: hex(0xffffff),
            on_attention: hex(0x17181b),
            hover: hex(0xeeeff1),
            sel: hex(0xe4ecfd),
            ok: hex(0x1f8a5b),
            bad: hex(0xc93a32),
            warn: hex(0xa77a00),
            add: rgba(31, 138, 91, 0.1),
            del: rgba(201, 58, 50, 0.09),
            highlight: rgba(255, 255, 255, 0.0),
            shadow: rgba(20, 24, 40, 0.14),
            r_pane: 0.,
            r_ctl: 6.,
            r_btn: 6.,
            r_pop: 10.,
            r_chip: 6.,
            gap: 0.,
            font: UI_FONT,
            mono: MONO_FONT,
            fs: 13.5,
        },
        (Style::Expressive, true) => Tokens {
            style,
            light,
            bg: hex(0xf3edf7),
            pane: hex(0xfef7ff),
            editor: hex(0xfef7ff),
            raised: hex(0xf3edf7),
            control: hex(0xece6f0),
            control2: hex(0xe6e0e9),
            border: hex(0xcac4d0),
            text: hex(0x1d1b20),
            muted: hex(0x49454f),
            faint: hex(0x79747e),
            accent: hex(0x6750a4),
            on_accent: hex(0xffffff),
            accent_soft: hex(0xeaddff),
            on_accent_soft: hex(0x4f378b),
            attention: hex(0xffd8e4),
            on_attention: hex(0x633b48),
            hover: hex(0xece6f0),
            sel: hex(0xe8def8),
            ok: hex(0x146c2e),
            bad: hex(0xb3261e),
            warn: hex(0x8b5000),
            add: rgba(20, 108, 46, 0.12),
            del: rgba(179, 38, 30, 0.1),
            highlight: rgba(255, 255, 255, 0.0),
            shadow: rgba(0, 0, 0, 0.12),
            r_pane: 28.,
            r_ctl: 14.,
            r_btn: 18.,
            r_pop: 16.,
            r_chip: 8.,
            gap: 8.,
            font: gpui_m3::theme::FONT_FAMILY,
            mono: MONO_FONT,
            fs: 13.5,
        },
        (Style::Expressive, false) => Tokens {
            style,
            light,
            bg: hex(0x211f26),
            pane: hex(0x141218),
            editor: hex(0x141218),
            raised: hex(0x211f26),
            control: hex(0x2b2930),
            control2: hex(0x36343b),
            border: hex(0x49454f),
            text: hex(0xe6e0e9),
            muted: hex(0xcac4d0),
            faint: hex(0x938f99),
            accent: hex(0xd0bcff),
            on_accent: hex(0x381e72),
            accent_soft: hex(0x4f378b),
            on_accent_soft: hex(0xeaddff),
            attention: hex(0x633b48),
            on_attention: hex(0xffd8e4),
            hover: hex(0x2b2930),
            sel: hex(0x4a4458),
            ok: hex(0xa8dab5),
            bad: hex(0xf2b8b5),
            warn: hex(0xffb868),
            add: rgba(168, 218, 181, 0.14),
            del: rgba(242, 184, 181, 0.13),
            highlight: rgba(255, 255, 255, 0.0),
            shadow: rgba(0, 0, 0, 0.4),
            r_pane: 28.,
            r_ctl: 14.,
            r_btn: 18.,
            r_pop: 16.,
            r_chip: 8.,
            gap: 8.,
            font: gpui_m3::theme::FONT_FAMILY,
            mono: MONO_FONT,
            fs: 13.5,
        },
        (Style::Glass, false) => Tokens {
            style,
            light,
            bg: rgba(12, 13, 18, 0.6),
            pane: rgba(40, 40, 48, 0.46),
            editor: rgba(18, 18, 24, 0.78),
            raised: rgba(32, 32, 38, 0.92),
            control: rgba(255, 255, 255, 0.1),
            control2: rgba(255, 255, 255, 0.18),
            border: rgba(255, 255, 255, 0.08),
            text: hex(0xffffff),
            muted: rgba(235, 235, 245, 0.62),
            faint: rgba(235, 235, 245, 0.36),
            accent: hex(0x0a84ff),
            on_accent: hex(0xffffff),
            accent_soft: rgba(10, 132, 255, 0.22),
            on_accent_soft: hex(0x64b0ff),
            attention: rgba(32, 32, 38, 0.92),
            on_attention: hex(0xffffff),
            hover: rgba(255, 255, 255, 0.08),
            sel: rgba(255, 255, 255, 0.12),
            ok: hex(0x30d158),
            bad: hex(0xff453a),
            warn: hex(0xff9f0a),
            add: rgba(48, 209, 88, 0.16),
            del: rgba(255, 69, 58, 0.16),
            highlight: rgba(255, 255, 255, 0.2),
            shadow: rgba(0, 0, 0, 0.45),
            r_pane: 18.,
            r_ctl: 999.,
            r_btn: 999.,
            r_pop: 22.,
            r_chip: 999.,
            gap: 8.,
            font: UI_FONT,
            mono: MONO_FONT,
            fs: 13.5,
        },
        (Style::Glass, true) => Tokens {
            style,
            light,
            bg: rgba(246, 246, 250, 0.5),
            pane: rgba(255, 255, 255, 0.58),
            editor: rgba(255, 255, 255, 0.82),
            raised: rgba(250, 250, 252, 0.94),
            control: rgba(255, 255, 255, 0.66),
            control2: rgba(255, 255, 255, 0.92),
            border: rgba(60, 60, 67, 0.12),
            text: hex(0x1d1d1f),
            muted: rgba(60, 60, 67, 0.76),
            faint: rgba(60, 60, 67, 0.44),
            accent: hex(0x007aff),
            on_accent: hex(0xffffff),
            accent_soft: rgba(0, 122, 255, 0.14),
            on_accent_soft: hex(0x0062cc),
            attention: rgba(250, 250, 252, 0.94),
            on_attention: hex(0x1d1d1f),
            hover: rgba(0, 0, 0, 0.045),
            sel: rgba(0, 0, 0, 0.065),
            ok: hex(0x248a3d),
            bad: hex(0xff3b30),
            warn: hex(0xc93400),
            add: rgba(52, 199, 89, 0.15),
            del: rgba(255, 59, 48, 0.12),
            highlight: rgba(255, 255, 255, 0.95),
            shadow: rgba(30, 40, 80, 0.16),
            r_pane: 18.,
            r_ctl: 999.,
            r_btn: 999.,
            r_pop: 22.,
            r_chip: 999.,
            gap: 8.,
            font: UI_FONT,
            mono: MONO_FONT,
            fs: 13.5,
        },
    }
}

static STYLE: AtomicU8 = AtomicU8::new(1);
static MODE: AtomicU8 = AtomicU8::new(0);
/// El claro/oscuro de Windows (para «Sistema»).
static SYSTEM_LIGHT: AtomicBool = AtomicBool::new(false);
/// El acento del espacio activo, `0xRRGGBB`; `NO_ACCENT` usa el del estilo.
static ACCENT: AtomicU32 = AtomicU32::new(NO_ACCENT);
const NO_ACCENT: u32 = u32::MAX;

pub fn set_system_light(light: bool) {
    SYSTEM_LIGHT.store(light, Ordering::Relaxed);
}

pub fn set_accent(accent: Option<u32>) {
    ACCENT.store(accent.unwrap_or(NO_ACCENT), Ordering::Relaxed);
}

pub fn accent() -> Option<u32> {
    Some(ACCENT.load(Ordering::Relaxed)).filter(|a| *a != NO_ACCENT)
}

/// Windows pide reducir el movimiento («Efectos de animación» apagados).
#[cfg(windows)]
pub fn system_reduced_motion() -> bool {
    windows::UI::ViewManagement::UISettings::new().and_then(|settings| settings.AnimationsEnabled()).map(|on| !on).unwrap_or(false)
}

#[cfg(not(windows))]
pub fn system_reduced_motion() -> bool {
    false
}

/// El acento de Formal y Liquid Glass (`simpleAccent` de la referencia): conserva
/// matiz y croma y sube o baja el tono para que contraste con el modo.
/// Devuelve el color y si el texto encima va negro.
pub fn simple_accent(rgb: u32, dark: bool) -> (u32, bool) {
    use material_colors::color::Argb;
    use material_colors::hct::Hct;
    let mut hct = Hct::new(Argb::new(255, (rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8));
    let tone = hct.get_tone();
    if dark && tone < 58. {
        hct.set_tone(62.);
    } else if !dark && tone > 62. {
        hct.set_tone(52.);
    }
    let argb: Argb = hct.into();
    let out = (argb.red as u32) << 16 | (argb.green as u32) << 8 | argb.blue as u32;
    (out, hct.get_tone() > 66.)
}

/// Los tokens con el acento del espacio.
fn with_accent(mut t: Tokens, rgb: u32) -> Tokens {
    if t.style == Style::Expressive {
        // M3: todo el esquema sale de la semilla (color dinámico, tonal spot).
        let s = gpui_m3::Scheme::from_seed(hex(rgb), !t.light);
        t.bg = s.surface_container;
        t.pane = s.surface;
        t.editor = s.surface;
        t.raised = s.surface_container;
        t.control = s.surface_container_high;
        t.control2 = s.surface_container_highest;
        t.border = s.outline_variant;
        t.text = s.on_surface;
        t.muted = s.on_surface_variant;
        t.faint = s.outline;
        t.accent = s.primary;
        t.on_accent = s.on_primary;
        t.accent_soft = s.primary_container;
        t.on_accent_soft = s.on_primary_container;
        t.attention = s.tertiary_container;
        t.on_attention = s.on_tertiary_container;
        t.hover = s.surface_container_high;
        t.sel = s.secondary_container;
        t.bad = s.error;
        return t;
    }
    let (accent, black_text) = simple_accent(rgb, !t.light);
    let color = hex(accent);
    t.accent = color;
    t.on_accent = if black_text { hex(0x000000) } else { hex(0xffffff) };
    t.accent_soft = color.opacity(if t.light { 0.13 } else { 0.22 });
    t.on_accent_soft = color;
    t.sel = color.opacity(if t.light { 0.16 } else { 0.30 });
    t
}

pub fn set(style: Style, mode: Mode) {
    STYLE.store(style as u8, Ordering::Relaxed);
    MODE.store(mode as u8, Ordering::Relaxed);
}

pub fn current() -> (Style, Mode) {
    let style = match STYLE.load(Ordering::Relaxed) {
        0 => Style::Formal,
        2 => Style::Glass,
        _ => Style::Expressive,
    };
    let mode = match MODE.load(Ordering::Relaxed) {
        1 => Mode::Light,
        2 => Mode::Dark,
        _ => Mode::System,
    };
    (style, mode)
}

/// Le pasa a gpui-m3 los colores del estilo y el modo de ahora. Expressive usa el esquema
/// de M3; Formal y Liquid Glass, uno armado con sus propios colores (`scheme_of`), para
/// que el markdown, los diffs y la salida de herramientas (que son de gpui-m3) se vean
/// como el resto de esos estilos.
pub fn apply_m3(cx: &mut gpui::App) {
    let t = settled();
    gpui_m3::Theme::set_scheme(m3_scheme(&t), !t.light, cx);
}

/// El esquema de gpui-m3 que corresponde a unos tokens (con el acento de ahora).
pub fn m3_scheme(t: &Tokens) -> gpui_m3::Scheme {
    if t.style == Style::Expressive {
        // Sin acento, la paleta base de M3, igual que en la referencia; con acento, su esquema.
        match accent() {
            Some(rgb) => gpui_m3::Scheme::from_seed(hex(rgb), !t.light),
            None if t.light => gpui_m3::Scheme::baseline_light(),
            None => gpui_m3::Scheme::baseline_dark(),
        }
    } else {
        scheme_of(t)
    }
}

/// Los colores de un estilo como un esquema de gpui-m3 (los nombres de cada campo
/// dicen qué variable de la referencia reemplaza).
pub fn scheme_of(t: &Tokens) -> gpui_m3::Scheme {
    // El «terciario» de los resaltados (tipos): un violeta que se lee en claro y oscuro.
    let tertiary: Hsla = if t.light { gpui::rgb(0x8250df).into() } else { gpui::rgb(0xc678dd).into() };
    gpui_m3::Scheme {
        primary: t.accent,
        on_primary: t.on_accent,
        primary_container: t.accent_soft,
        on_primary_container: t.on_accent_soft,
        secondary_container: t.control,
        on_secondary_container: t.text,
        tertiary,
        tertiary_container: t.attention,
        on_tertiary_container: t.on_attention,
        surface: t.pane,
        surface_container: t.raised,
        surface_container_high: t.control,
        surface_container_highest: t.control2,
        on_surface: t.text,
        on_surface_variant: t.muted,
        outline: t.faint,
        outline_variant: t.border,
        error: t.bad,
        error_container: t.del,
        on_error_container: t.bad,
        success: t.ok,
        warning: t.warn,
        shadow: t.shadow,
    }
}

/// Los tokens de ahora.
pub fn t() -> Tokens {
    let now = settled();
    // Al cambiar de acento o de modo en Expressive, los colores se mezclan del anterior al
    // nuevo en vez de saltar (`motion.css:9-84`): mientras dura, `t()` da la mezcla.
    let Ok(mut blend) = BLEND.lock() else {
        return now;
    };
    let Some(running) = *blend else {
        return now;
    };
    let progress = running.start.elapsed().as_secs_f32() / running.secs;
    if progress >= 1. {
        *blend = None;
        return now;
    }
    mix_tokens(&now, &running.from, gpui_m3::motion::cubic_bezier(0.2, 0., 0., 1.)(progress))
}

/// Los tokens del estilo y el modo de ahora, sin mezcla.
pub fn settled() -> Tokens {
    let (style, mode) = current();
    resolved(style, mode)
}

/// Los tokens de un estilo y un modo (con el acento de ahora; «Sistema» según Windows).
pub fn resolved(style: Style, mode: Mode) -> Tokens {
    let light = match mode {
        Mode::System => SYSTEM_LIGHT.load(Ordering::Relaxed),
        Mode::Light => true,
        Mode::Dark => false,
    };
    let base = tokens(style, light);
    let Some(rgb) = accent() else {
        return base;
    };
    // El esquema de M3 cuesta: se guarda el último.
    static CACHE: std::sync::Mutex<Option<((Style, bool, u32), Tokens)>> = std::sync::Mutex::new(None);
    let key = (style, light, rgb);
    if let Ok(mut cache) = CACHE.lock() {
        if let Some((cached, tokens)) = *cache {
            if cached == key {
                return tokens;
            }
        }
        let tokens = with_accent(base, rgb);
        *cache = Some((key, tokens));
        return tokens;
    }
    with_accent(base, rgb)
}

/// Una mezcla de colores en curso: de dónde viene, cuándo empezó y cuánto dura.
#[derive(Clone, Copy)]
struct Blend {
    from: Tokens,
    start: std::time::Instant,
    secs: f32,
}

static BLEND: std::sync::Mutex<Option<Blend>> = std::sync::Mutex::new(None);

/// Empieza a mezclar desde `from` hacia los tokens de ahora durante `secs` segundos.
pub fn begin_blend(from: Tokens, secs: f32) {
    if let Ok(mut blend) = BLEND.lock() {
        *blend = Some(Blend { from, start: std::time::Instant::now(), secs: secs.max(0.01) });
    }
}

/// Corta la mezcla de colores en curso (un cambio de estilo no se mezcla).
pub fn end_blend() {
    if let Ok(mut blend) = BLEND.lock() {
        *blend = None;
    }
}

/// Si los colores se están mezclando (la vista pide cuadros mientras tanto).
pub fn blending() -> bool {
    BLEND.lock().is_ok_and(|blend| blend.is_some_and(|b| b.start.elapsed().as_secs_f32() < b.secs))
}

/// Cada color entre `from` (con `p = 0`) y `to` (con `p = 1`); la forma y las medidas son las de `to`.
fn mix_tokens(to: &Tokens, from: &Tokens, p: f32) -> Tokens {
    use gpui_m3::theme::mix;
    let mut out = *to;
    macro_rules! blend {
        ($($field:ident),* $(,)?) => { $(out.$field = mix(to.$field, from.$field, p);)* };
    }
    blend!(
        bg, pane, editor, raised, control, control2, border, text, muted, faint, accent, on_accent, accent_soft,
        on_accent_soft, attention, on_attention, hover, sel, ok, bad, warn, add, del, highlight, shadow
    );
    out
}

/// `#rrggbb` → `0xRRGGBB`.
pub fn parse_hex(text: &str) -> Option<u32> {
    let digits = text.trim().trim_start_matches('#');
    (digits.len() == 6).then(|| u32::from_str_radix(digits, 16).ok()).flatten()
}

pub fn to_hex(rgb: u32) -> String {
    format!("#{rgb:06x}")
}

/// Un color de GPUI como `0xRRGGBB`.
pub fn rgb_of(color: Hsla) -> u32 {
    let c = Rgba::from(color);
    let byte = |v: f32| (v.clamp(0., 1.) * 255.).round() as u32;
    byte(c.r) << 16 | byte(c.g) << 8 | byte(c.b)
}

pub fn hsla_of(rgb: u32) -> Hsla {
    hex(rgb)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(rgb: u32) -> f64 {
        use material_colors::{color::Argb, hct::Hct};
        Hct::new(Argb::new(255, (rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)).get_tone()
    }

    #[test]
    fn el_acento_simple_ajusta_el_tono_al_modo() {
        // Oscuro con un azul marino (tono bajo): sube a 62 para que se lea.
        let (dark, black) = simple_accent(0x1a237e, true);
        assert!((tone(dark) - 62.).abs() < 1.5, "tono {}", tone(dark));
        assert!(!black);
        // Claro con un amarillo (tono alto): baja a 52.
        let (light, _) = simple_accent(0xffcc00, false);
        assert!((tone(light) - 52.).abs() < 1.5, "tono {}", tone(light));
        // El mismo amarillo en oscuro se deja, y el texto encima va negro.
        assert_eq!(simple_accent(0xffcc00, true), (0xffcc00, true));
        // Un tono intermedio no se toca.
        assert_eq!(simple_accent(0x2563eb, false).0, 0x2563eb);
    }

    #[test]
    fn los_colores_van_y_vuelven_en_hex() {
        assert_eq!(parse_hex("#6750A4"), Some(0x6750a4));
        assert_eq!(parse_hex("6750a4"), Some(0x6750a4));
        assert_eq!(parse_hex("#fff"), None);
        assert_eq!(to_hex(0x0b57d0), "#0b57d0");
        assert_eq!(rgb_of(hsla_of(0x386a20)), 0x386a20);
    }
}
