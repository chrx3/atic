//! El tema de las ventanas: Atic Code, Ajustes y Reuniones.
//!
//! Todas las ventanas usan la misma paleta oscura, y sus colores viven aquí
//! con un nombre (`WINDOW`, `SURFACE`, `TEXT`…). Cada color oscuro tiene su
//! par claro en `LIGHT`: `hsla` devuelve el que toca según el tema. Un color
//! que no está en la tabla (los acentos de cada agente, los diffs, lo que
//! pinta un programa en su terminal) sale igual en los dos.
//!
//! Las terminales y el código siguen oscuros en el tema claro: los programas
//! eligen sus colores pensando en un fondo oscuro. Por eso el fondo de la
//! terminal (`console::BACKGROUND`) no está en la tabla, y lo que en la
//! interfaz se hunde un poco usa `SUNKEN`, que sí cambia.
//!
//! La pill y sus paneles flotantes no pasan por aquí: siguen oscuros, se ven
//! sobre cualquier fondo.
//!
//! El tema sale de `ui_theme` en `config.json` (el mismo de Atic): `system`
//! sigue a Windows («Modo de aplicación» claro u oscuro).
//!
//! El estilo de las ventanas se elige en Ajustes › Apariencia
//! (`appearance::WindowStyle`): con «Atic», la paleta de siempre; con M3
//! Expressive o Liquid Glass, cada color de la tabla toma el papel que le toca
//! en los tokens de ese estilo de Atic Code (`ROLES`). El claro u oscuro sigue
//! saliendo de `ui_theme`.

use std::sync::atomic::{AtomicBool, Ordering};

use gpui::{Hsla, Rgba};

use crate::code::style::{Style, Tokens};

// --- Colores con nombre (su valor oscuro) -----------------------------------------

pub const WINDOW: u32 = 0x0f0f0e;
/// Lo que se hunde un poco bajo un panel: el fondo de unas pestañas, una barra.
pub const SUNKEN: u32 = 0x171716;
pub const SURFACE: u32 = 0x1d1d1b;
pub const SURFACE_HOVER: u32 = 0x252523;
pub const ITEM: u32 = 0x262624;
pub const SURFACE_ON: u32 = 0x2d2d2a;
pub const TEXT: u32 = 0xf0f0ea;
pub const BODY: u32 = 0xd9d9d1;
pub const MUTED: u32 = 0x9a9a90;
pub const FAINT: u32 = 0x6a6a64;
/// La letra sobre lo claro (el botón principal, una pestaña elegida).
pub const INK: u32 = 0x141413;
/// El botón principal y la pestaña elegida.
pub const FILL: u32 = 0xe9e9e2;
pub const AMBER: u32 = 0xe8b04b;
pub const GREEN: u32 = 0x6cc48a;

/// Cada color oscuro de la interfaz y su par en el tema claro: cálidos, como
/// el papel de Atic.
const LIGHT: &[(u32, u32)] = &[
    // Fondos, de más hondo a más alto. En claro, los paneles son blancos sobre
    // una ventana de papel.
    (WINDOW, 0xf2f0ea),
    (0x131312, 0xe9e6de),
    (0x151514, 0xeeebe4),
    (SUNKEN, 0xe6e3db),
    (0x1b1b19, 0xfbfaf7),
    (SURFACE, 0xffffff),
    (0x1f1f1d, 0xe7e4dc),
    (0x232321, 0xeeebe4),
    (SURFACE_HOVER, 0xf4f2ed),
    (ITEM, 0xf4f2ed),
    (0x2a2a28, 0xebe8e1),
    (0x2b2b28, 0xe9e6de),
    (SURFACE_ON, 0xe6e3db),
    (0x2c2c2a, 0xe6e3db),
    (0x2e2e2b, 0xe2dfd7),
    (0x323230, 0xeeebe4),
    (0x33332f, 0xe6e3db),
    (0x343431, 0xdcd9d0),
    (0x353532, 0xdedbd2),
    (0x3a3a36, 0xd6d3ca),
    (0x3a3a37, 0xd6d3ca),
    (0x3e3e39, 0xd9d6cd),
    (0x46463f, 0xcfccc3),
    // Reuniones: el tenue de un bloque con el cursor encima y el del que suena
    // (sin estos quedaban casi negros en claro), el globito de la onda y la tarjeta
    // del vistazo, el campo de una línea, el aviso de deshacer y el botón de
    // velocidad elegido.
    (0x222220, 0xf6f4ef),
    (0x2b2b29, 0xebe8e1),
    (0x2a2a27, 0xfbfaf7),
    (0x31312e, 0xf4f2ed),
    (0x363633, 0xfbfaf7),
    (0x444440, 0xcfccc3),
    // Letra.
    (TEXT, 0x1c1b18),
    (BODY, 0x33322d),
    (MUTED, 0x6b6a62),
    (0x6a6a64, 0x97958c),
    (0x5a5a54, 0xa3a198),
    // Lo claro sobre lo oscuro se da vuelta: el botón principal es oscuro.
    (FILL, 0x1c1b18),
    (0xffffff, 0x34332e),
    (INK, 0xf7f6f2),
    // Acentos: un poco más hondos, para leerse sobre blanco.
    (AMBER, 0xb98114),
    (GREEN, 0x3b9a5e),
    (0xf07b6e, 0xc9473a),
    (0xe5705f, 0xc9473a),
    (0xffa89b, 0xb23a2c),
    (0x3b2321, 0xf8e0dc),
    (0x4a2b28, 0xf2cec8),
    (0xf0a497, 0xb23a2c),
];

static LIGHT_ON: AtomicBool = AtomicBool::new(false);
/// Cambió el tema y las ventanas tienen que volver a dibujarse.
static CHANGED: AtomicBool = AtomicBool::new(false);

pub fn is_light() -> bool {
    LIGHT_ON.load(Ordering::Relaxed)
}

/// El color que toca en el tema de ahora.
pub fn resolve(color: u32) -> u32 {
    if !is_light() {
        return color;
    }
    LIGHT.iter().find(|(dark, _)| *dark == color).map_or(color, |(_, light)| *light)
}

pub fn hsla(color: u32) -> Hsla {
    if let Some(tokens) = styled() {
        if let Some((_, role)) = ROLES.iter().find(|(dark, _)| *dark == color) {
            return role.pick(&tokens);
        }
    }
    raw(resolve(color))
}

/// Qué es cada color de la tabla en los estilos de Atic Code.
#[derive(Clone, Copy)]
enum Role {
    Window,
    Card,
    Hover,
    Control,
    ControlHigh,
    Border,
    Text,
    Muted,
    Faint,
    Fill,
    OnFill,
    Warn,
    Ok,
    Bad,
    BadSoft,
}

impl Role {
    fn pick(self, t: &Tokens) -> Hsla {
        match self {
            // En M3 la ventana es la superficie y las tarjetas el contenedor; en el
            // vidrio, el fondo translúcido sobre el material del sistema.
            Role::Window if t.style == Style::Expressive => t.pane,
            // En el vidrio el fondo lo pone la ventana (`window_bg`): lo que dentro
            // usa su color queda transparente, si no se taparía dos veces.
            Role::Window => gpui::transparent_black(),
            Role::Card if t.style == Style::Glass => t.pane,
            Role::Card => t.raised,
            Role::Hover => t.hover,
            Role::Control => t.control,
            Role::ControlHigh => t.control2,
            Role::Border => t.border,
            Role::Text => t.text,
            Role::Muted => t.muted,
            Role::Faint => t.faint,
            Role::Fill => t.accent,
            Role::OnFill => t.on_accent,
            Role::Warn => t.warn,
            Role::Ok => t.ok,
            Role::Bad => t.bad,
            Role::BadSoft => t.del,
        }
    }
}

/// El papel de cada color oscuro de `LIGHT`.
const ROLES: &[(u32, Role)] = &[
    (WINDOW, Role::Window),
    (0x131312, Role::Control),
    (0x151514, Role::Hover),
    (SUNKEN, Role::Control),
    (0x1b1b19, Role::Card),
    (SURFACE, Role::Card),
    (0x1f1f1d, Role::Control),
    (0x232321, Role::Hover),
    (SURFACE_HOVER, Role::Hover),
    (ITEM, Role::Control),
    (0x2a2a28, Role::Control),
    (0x2b2b28, Role::Control),
    (SURFACE_ON, Role::ControlHigh),
    (0x2c2c2a, Role::ControlHigh),
    (0x2e2e2b, Role::ControlHigh),
    (0x323230, Role::Hover),
    (0x33332f, Role::ControlHigh),
    (0x343431, Role::Border),
    (0x353532, Role::Border),
    (0x3a3a36, Role::Border),
    (0x3a3a37, Role::Border),
    (0x3e3e39, Role::Border),
    (0x46463f, Role::Border),
    (0x222220, Role::Hover),
    (0x2b2b29, Role::Control),
    (0x2a2a27, Role::Card),
    (0x31312e, Role::Hover),
    (0x363633, Role::Card),
    (0x444440, Role::Border),
    (TEXT, Role::Text),
    (BODY, Role::Text),
    (MUTED, Role::Muted),
    (0x6a6a64, Role::Faint),
    (0x5a5a54, Role::Faint),
    (FILL, Role::Fill),
    (0xffffff, Role::Text),
    (INK, Role::OnFill),
    (AMBER, Role::Warn),
    (GREEN, Role::Ok),
    (0xf07b6e, Role::Bad),
    (0xe5705f, Role::Bad),
    (0xffa89b, Role::Bad),
    (0x3b2321, Role::BadSoft),
    (0x4a2b28, Role::BadSoft),
    (0xf0a497, Role::Bad),
];

/// Los tokens del estilo de las ventanas, o `None` con «Atic» (la paleta de siempre).
/// En Liquid Glass los fondos grandes siguen la transparencia elegida.
pub fn styled() -> Option<Tokens> {
    let look = crate::appearance::current();
    let mut t = crate::code::style::tokens(look.windows.code_style()?, is_light());
    if t.style == Style::Glass {
        // 50 % deja los fondos como en Atic Code; 0 %, solo el material del sistema.
        let factor = look.glass_opacity.clamp(0., 1.) * 2.;
        for color in [&mut t.bg, &mut t.pane, &mut t.editor] {
            color.a = (color.a * factor).min(1.);
        }
    }
    Some(t)
}

/// La letra de la ventana según el estilo.
pub fn font() -> &'static str {
    styled().map_or("Segoe UI", |t| t.font)
}

/// Mientras se dibuja una ventana con estilo, los componentes de gpui-m3 usan
/// el esquema de ese estilo y no el de Atic Code: su tema es uno solo para toda
/// la app. Devuelve la capa que, pintada al final, deja el de Atic Code como
/// estaba. Con «Atic» no hace nada.
pub fn scope_m3(cx: &mut gpui::App) -> Option<gpui::AnyElement> {
    use gpui::{canvas, deferred, IntoElement};
    let t = styled()?;
    let scheme = match (t.style, t.light) {
        (Style::Expressive, true) => gpui_m3::Scheme::baseline_light(),
        (Style::Expressive, false) => gpui_m3::Scheme::baseline_dark(),
        _ => crate::code::style::scheme_of(&t),
    };
    let previous = gpui_m3::Theme::get(cx);
    cx.set_global(gpui_m3::Theme { scheme, dark: !t.light, seed: None });
    let restore = canvas(|_, _, _| {}, move |_, _, _, cx| cx.set_global(previous));
    Some(deferred(restore).with_priority(usize::MAX).into_any_element())
}

/// El fondo de toda la ventana (su raíz).
pub fn window_bg() -> Hsla {
    match styled() {
        Some(t) if t.style == Style::Glass => t.bg,
        _ => hsla(WINDOW),
    }
}

/// El radio de una tarjeta: el de los menús y tarjetas del estilo, o `default`.
pub fn card_radius(default: f32) -> f32 {
    styled().map_or(default, |t| t.r_pop)
}

/// Pone o quita el material del sistema detrás de la ventana cuando el estilo
/// pasa a Liquid Glass o sale de él. `shown` recuerda lo último puesto (el claro
/// u oscuro del vidrio), para no tocar la ventana en cada cuadro.
pub fn sync_backdrop(window: &mut gpui::Window, shown: &mut Option<bool>) {
    let want = styled().filter(|t| t.style == Style::Glass).map(|t| t.light);
    if *shown == want {
        return;
    }
    *shown = want;
    crate::code::set_glass_background(want.is_some(), want.unwrap_or(false), window);
}

/// Cambió el estilo de las ventanas: tienen que volver a dibujarse.
pub fn restyle() {
    CHANGED.store(true, Ordering::Relaxed);
}

/// Sin pasar por el tema: lo que pinta una terminal.
pub fn raw(color: u32) -> Hsla {
    Rgba {
        r: (color >> 16 & 0xff) as f32 / 255.0,
        g: (color >> 8 & 0xff) as f32 / 255.0,
        b: (color & 0xff) as f32 / 255.0,
        a: 1.0,
    }
    .into()
}

/// ¿Es claro este `ui_theme` de Atic? `system` (o uno desconocido) pregunta a
/// Windows.
pub fn wants_light(ui_theme: &str) -> bool {
    match ui_theme {
        "light" | "sepia" | "mist" | "claude" => true,
        "dark" | "graphite" | "midnight" | "claude-dark" => false,
        _ => windows_light(),
    }
}

/// Aplica el tema; avisa para redibujar solo si cambió.
pub fn set_light(light: bool) {
    if LIGHT_ON.swap(light, Ordering::Relaxed) != light {
        CHANGED.store(true, Ordering::Relaxed);
    }
}

/// Lo consume el bucle de la pill para redibujar todas las ventanas.
pub fn take_changed() -> bool {
    CHANGED.swap(false, Ordering::Relaxed)
}

/// El «Modo de aplicación» de Windows: `AppsUseLightTheme` en el registro.
#[cfg(windows)]
pub fn windows_light() -> bool {
    use windows::core::w;
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
    let mut value: u32 = 0;
    let mut size = std::mem::size_of::<u32>() as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize"),
            w!("AppsUseLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut value as *mut u32 as *mut _),
            Some(&mut size),
        )
    };
    status.is_ok() && value == 1
}

#[cfg(not(windows))]
pub fn windows_light() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cada_color_oscuro_tiene_un_solo_par_claro() {
        for (index, (dark, _)) in LIGHT.iter().enumerate() {
            assert!(!LIGHT[index + 1..].iter().any(|(other, _)| other == dark), "{dark:06x} repetido");
        }
    }

    #[test]
    fn cada_color_con_par_claro_tiene_papel_en_los_estilos() {
        for (dark, _) in LIGHT {
            assert!(ROLES.iter().any(|(other, _)| other == dark), "{dark:06x} sin papel");
        }
    }

    #[test]
    fn la_terminal_no_cambia_de_fondo() {
        assert!(!LIGHT.iter().any(|(dark, _)| *dark == 0x161615));
    }

    #[test]
    fn los_temas_de_atic_se_reparten_en_claro_y_oscuro() {
        assert!(wants_light("sepia"));
        assert!(wants_light("claude"));
        assert!(!wants_light("midnight"));
        assert!(!wants_light("claude-dark"));
    }
}
