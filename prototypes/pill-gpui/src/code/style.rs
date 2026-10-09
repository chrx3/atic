//! Los estilos de Atic Code, los mismos de la referencia (`src/styles/tokens.css`):
//! Formal, Material 3 Expressive y Liquid Glass, cada uno en claro y oscuro.
//! La vista solo usa estos nombres; cada estilo pone valores, forma y si los
//! paneles flotan.
//!
//! El modo sale del tema de Atic (`crate::theme::is_light`) salvo que se fije
//! claro u oscuro. Liquid Glass hace la ventana translúcida sobre el escritorio
//! desenfocado (Acrylic de Windows); GPUI no desenfoca lo que queda detrás de
//! un panel dentro de la ventana, así que los paneles son vidrio sobre ese fondo.

use std::sync::atomic::{AtomicU8, Ordering};

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
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// El de Atic.
    #[default]
    System,
    Light,
    Dark,
}

impl Mode {
    pub const ALL: [(Mode, &'static str); 3] = [(Mode::System, "Como Atic"), (Mode::Light, "Claro"), (Mode::Dark, "Oscuro")];
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

/// Le pasa a gpui-m3 los colores de Expressive en el modo de ahora. Solo
/// Expressive usa sus componentes; con los otros estilos no hace falta.
pub fn apply_m3(cx: &mut gpui::App) {
    let t = t();
    if t.style == Style::Expressive {
        // Los tokens de Expressive son los de la paleta base de M3, igual que en la referencia.
        let scheme = if t.light { gpui_m3::Scheme::baseline_light() } else { gpui_m3::Scheme::baseline_dark() };
        gpui_m3::Theme::set_scheme(scheme, !t.light, cx);
    }
}

/// Los tokens de ahora.
pub fn t() -> Tokens {
    let (style, mode) = current();
    let light = match mode {
        Mode::System => crate::theme::is_light(),
        Mode::Light => true,
        Mode::Dark => false,
    };
    tokens(style, light)
}
