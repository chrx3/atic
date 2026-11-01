//! Los colores, radios y letras del editor según el estilo de Apariencia: con
//! «Atic», la paleta de siempre; con Material 3 Expressive o Liquid Glass, los
//! tokens de ese estilo (`theme::styled`). Con Liquid Glass por piezas (macOS
//! 26), los paneles son vidrios del sistema (`glass_pieces`).

use gpui::{Div, Hsla, ParentElement, Rgba, Styled};

use crate::code::style::Style;
use crate::theme;

// --- La escala tipográfica: cuatro tamaños con nombre, sin medios píxeles ----------------------

/// Títulos de sección en mayúsculas, notas, etiquetas de pistas y teclas.
pub const T_SMALL: f32 = 11.0;
/// Etiquetas de campo, valores, opciones (chips) y avisos.
pub const T_BODY: f32 = 12.0;
/// Botones, campos y filas.
pub const T_CONTROL: f32 = 13.0;
/// El título de un panel o de un diálogo.
pub const T_TITLE: f32 = 14.0;

/// Lo mínimo que contrasta el texto secundario con lo que tiene detrás (WCAG AA).
pub const MIN_CONTRAST: f32 = 4.5;
/// El borde del anillo de foco de teclado (px). Los elementos enfocables llevan un borde
/// transparente de este grosor para no moverse al enfocarse.
pub const RING_W: f32 = 2.0;

#[derive(Clone, Copy)]
pub struct Ed {
    pub light: bool,
    /// Los paneles son vidrios del sistema (no se pinta su fondo).
    pub glass: bool,
    pub expressive: bool,
    /// El fondo de la ventana.
    pub bg: Hsla,
    /// Paneles (la barra lateral, la línea de tiempo).
    pub panel: Hsla,
    /// Lo que va encima de un panel: tarjetas, menús, la vista previa.
    pub raised: Hsla,
    /// El fondo de un control (un botón, una pista vacía).
    pub control: Hsla,
    pub border: Hsla,
    pub text: Hsla,
    pub muted: Hsla,
    pub faint: Hsla,
    /// El texto secundario que se tiene que leer (etiquetas, notas, valores): `muted` con
    /// lo justo de `text` para llegar a `MIN_CONTRAST` sobre un control. `faint` queda para
    /// íconos y lo decorativo.
    pub sub: Hsla,
    pub accent: Hsla,
    pub on_accent: Hsla,
    pub hover: Hsla,
    pub sel: Hsla,
    pub r_panel: f32,
    pub r_ctl: f32,
    pub r_chip: f32,
    pub font: &'static str,
    pub mono: &'static str,
    /// Las pistas: zooms, textos, cortes, clics y el recorte.
    pub zoom: Hsla,
    pub text_lane: Hsla,
    pub cut: Hsla,
    pub click: Hsla,
    pub trim: Hsla,
    pub speed: Hsla,
    pub blur: Hsla,
}

/// Cuánto se ve un control en reposo sobre el panel (lo mismo que las filas de opciones).
const CONTROL_REST: f32 = 0.55;

/// Los colores de las pistas de velocidad y de difuminado (datos, como los de zoom y texto).
const SPEED_COLOR: u32 = 0x4cc9b0;
const BLUR_COLOR: u32 = 0xd77ad0;

impl Ed {
    pub fn current() -> Self {
        let zoom = gpui::rgb(0x9b87f5).into();
        let text_lane = gpui::rgb(0x5fb3e6).into();
        let cut = theme::hsla(0xf07b6e);
        let speed = gpui::rgb(SPEED_COLOR).into();
        let blur = gpui::rgb(BLUR_COLOR).into();
        let mut ed = match theme::styled() {
            Some(t) => Self {
                light: t.light,
                glass: crate::glass_pieces::active(t.style),
                expressive: t.style == Style::Expressive,
                bg: theme::window_bg(),
                panel: t.pane,
                raised: t.raised,
                control: t.control,
                border: t.border,
                text: t.text,
                muted: t.muted,
                faint: t.faint,
                sub: t.muted,
                accent: t.accent,
                on_accent: t.on_accent,
                hover: t.hover,
                sel: t.sel,
                r_panel: t.r_pane,
                r_ctl: t.r_ctl,
                r_chip: t.r_chip,
                font: t.font,
                mono: t.mono,
                zoom,
                text_lane,
                cut,
                click: t.warn,
                trim: t.warn,
                speed,
                blur,
            },
            None => Self {
                light: theme::is_light(),
                glass: false,
                expressive: false,
                bg: theme::hsla(theme::WINDOW),
                panel: theme::hsla(theme::SURFACE),
                raised: theme::hsla(theme::SUNKEN),
                control: theme::hsla(theme::ITEM),
                border: theme::hsla(0x444440).opacity(0.6),
                text: theme::hsla(theme::TEXT),
                muted: theme::hsla(theme::MUTED),
                faint: theme::hsla(theme::FAINT),
                sub: theme::hsla(theme::MUTED),
                accent: theme::hsla(theme::FILL),
                on_accent: theme::hsla(theme::INK),
                hover: theme::hsla(theme::SURFACE_HOVER),
                sel: theme::hsla(theme::SURFACE_ON),
                r_panel: 14.0,
                r_ctl: 10.0,
                r_chip: 14.0,
                font: crate::fonts::UI,
                mono: crate::fonts::MONO,
                zoom,
                text_lane,
                cut,
                click: theme::hsla(theme::AMBER),
                trim: theme::hsla(theme::AMBER),
                speed,
                blur,
            },
        };
        ed.sub = readable(ed.muted, ed.surface(), ed.text, MIN_CONTRAST);
        ed
    }

    /// Un fondo de referencia para medir contraste: un control (el peor caso, el más claro u
    /// oscuro de los fondos de texto de la barra) sobre el panel sobre la ventana.
    pub fn surface(&self) -> Hsla {
        over(self.control.opacity(CONTROL_REST), over(self.panel, over(self.bg, opaque_base(self.light))))
    }

    /// El fondo de un panel: un vidrio con Liquid Glass por piezas; si no, `panel`
    /// con un borde fino. Va antes de los hijos.
    pub fn panel(&self, el: Div, radius: f32) -> Div {
        if self.glass {
            el.rounded(gpui::px(radius)).child(crate::glass_pieces::slot(crate::glass_pieces::Piece::Glass { radius }))
        } else {
            el.rounded(gpui::px(radius)).bg(self.panel).border_1().border_color(self.border.opacity(0.5))
        }
    }

    /// La raíz de la ventana.
    pub fn root(&self, el: Div) -> Div {
        crate::glass_pieces::root(el, self.bg)
    }
}


// --- Contraste ---------------------------------------------------------------------------------

/// Lo que hay detrás de la ventana si el fondo es translúcido: blanco o negro según el tema.
fn opaque_base(light: bool) -> Hsla {
    if light {
        gpui::white()
    } else {
        gpui::black()
    }
}

/// `fg` sobre `bg`, con el alfa de `fg`; el resultado es opaco.
pub fn over(fg: Hsla, bg: Hsla) -> Hsla {
    let (f, b) = (fg.to_rgb(), bg.to_rgb());
    let a = f.a.clamp(0.0, 1.0);
    Rgba { r: f.r * a + b.r * (1.0 - a), g: f.g * a + b.g * (1.0 - a), b: f.b * a + b.b * (1.0 - a), a: 1.0 }.into()
}

/// Mezcla lineal de dos colores en RGB (`t` de 0 a 1).
fn blend(a: Hsla, b: Hsla, t: f32) -> Hsla {
    let (a, b) = (a.to_rgb(), b.to_rgb());
    Rgba { r: a.r + (b.r - a.r) * t, g: a.g + (b.g - a.g) * t, b: a.b + (b.b - a.b) * t, a: 1.0 }.into()
}

/// La luminancia relativa de WCAG (0 negro, 1 blanco).
pub fn luminance(color: Hsla) -> f32 {
    let c = color.to_rgb();
    let lin = |v: f32| if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) };
    0.2126 * lin(c.r) + 0.7152 * lin(c.g) + 0.0722 * lin(c.b)
}

/// La razón de contraste de WCAG entre dos colores opacos (1 a 21).
pub fn contrast(a: Hsla, b: Hsla) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// `fg` (sobre `bg`) acercado a `toward` lo justo para contrastar `min` con `bg`; si ya llega,
/// queda como está. Así el texto secundario no inventa un color: es el mismo con algo más de fuerza.
pub fn readable(fg: Hsla, bg: Hsla, toward: Hsla, min: f32) -> Hsla {
    let (fg, bg) = (over(fg, bg), over(bg, opaque_base(false)));
    if contrast(fg, bg) >= min {
        return fg;
    }
    let toward = over(toward, bg);
    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    for _ in 0..14 {
        let mid = (lo + hi) / 2.0;
        if contrast(blend(fg, toward, mid), bg) >= min {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    blend(fg, toward, hi)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blanco_sobre_negro_es_el_contraste_maximo() {
        assert!((contrast(gpui::white(), gpui::black()) - 21.0).abs() < 0.01);
        assert!((contrast(gpui::white(), gpui::white()) - 1.0).abs() < 1e-3);
    }

    #[test]
    fn el_texto_tenue_sube_hasta_el_contraste_minimo_en_claro_y_en_oscuro() {
        // Un gris tenue (el `faint` de siempre) sobre un control oscuro y sobre uno claro.
        let dark_bg: Hsla = gpui::rgb(0x2c2c2a).into();
        let dim: Hsla = gpui::rgb(0x6a6a64).into();
        assert!(contrast(dim, dark_bg) < MIN_CONTRAST, "el punto de partida no llegaba");
        let fixed = readable(dim, dark_bg, gpui::rgb(0xf0f0ea).into(), MIN_CONTRAST);
        assert!(contrast(fixed, dark_bg) >= MIN_CONTRAST - 0.01);
        let light_bg: Hsla = gpui::rgb(0xe6e6e0).into();
        let pale: Hsla = gpui::rgb(0x9a9a90).into();
        assert!(contrast(pale, light_bg) < MIN_CONTRAST);
        let fixed = readable(pale, light_bg, gpui::rgb(0x1a1a18).into(), MIN_CONTRAST);
        assert!(contrast(fixed, light_bg) >= MIN_CONTRAST - 0.01);
    }

    #[test]
    fn lo_que_ya_contrasta_no_se_toca() {
        let bg: Hsla = gpui::rgb(0x2c2c2a).into();
        let fg: Hsla = gpui::rgb(0xe0e0da).into();
        let out = readable(fg, bg, gpui::white(), MIN_CONTRAST);
        assert!((luminance(out) - luminance(fg)).abs() < 1e-4);
    }

    #[test]
    fn un_color_translucido_se_mide_sobre_lo_que_tiene_detras() {
        let out = over(gpui::white().opacity(0.5), gpui::black());
        assert!((luminance(out) - luminance(gpui::rgb(0x808080).into())).abs() < 0.01);
    }

    #[test]
    fn la_escala_tipografica_sube_de_a_un_paso() {
        assert!(T_SMALL < T_BODY && T_BODY < T_CONTROL && T_CONTROL < T_TITLE);
        for size in [T_SMALL, T_BODY, T_CONTROL, T_TITLE] {
            assert_eq!(size.fract(), 0.0, "sin medios píxeles");
        }
    }
}
