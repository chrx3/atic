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

use std::sync::atomic::{AtomicBool, Ordering};

use gpui::{Hsla, Rgba};

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
    raw(resolve(color))
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
