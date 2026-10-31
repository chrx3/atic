//! Las letras de la pill en cada sistema: Segoe UI y Cascadia en Windows;
//! la del sistema (San Francisco) y Menlo en macOS.

#[cfg(not(target_os = "macos"))]
mod names {
    pub const UI: &str = "Segoe UI";
    pub const UI_TEXT: &str = "Segoe UI Variable Text";
    pub const MONO: &str = "Cascadia Mono";
    pub const MONO_ALT: &str = "Consolas";
    pub const HAND: &str = "Segoe Print";
    pub const EMOJI: &str = "Segoe UI Emoji";
    pub const ICONS: &str = "Segoe Fluent Icons";
}

#[cfg(target_os = "macos")]
mod names {
    /// San Francisco, con el nombre que entiende GPUI en Mac.
    pub const UI: &str = ".SystemUIFont";
    pub const UI_TEXT: &str = ".SystemUIFont";
    pub const MONO: &str = "Menlo";
    pub const MONO_ALT: &str = "Menlo";
    pub const HAND: &str = "Noteworthy";
    pub const EMOJI: &str = "Apple Color Emoji";
    /// Sin Segoe Fluent Icons: los íconos caen a la letra del sistema.
    pub const ICONS: &str = ".SystemUIFont";
}

pub use names::*;

/// La letra normal de la Pizarra: la misma que dibuja `files(Kind::Sans)`,
/// así el editor y la exportación se ven iguales.
pub const BOARD_SANS: &str = if cfg!(target_os = "macos") { "Helvetica Neue" } else { "Segoe UI" };

/// Los archivos de una letra para dibujarla a mano (la Pizarra), en orden de
/// preferencia.
pub fn files(kind: Kind) -> Vec<std::path::PathBuf> {
    use std::path::PathBuf;
    if cfg!(target_os = "macos") {
        let names: &[&str] = match kind {
            // SF (`SFNS.ttf`) es una fuente variable cuyos contornos no lee
            // `ttf-parser` sin elegir un peso: Helvetica Neue en su lugar.
            Kind::Sans => &["/System/Library/Fonts/HelveticaNeue.ttc", "/System/Library/Fonts/Helvetica.ttc"],
            Kind::Mono => &["/System/Library/Fonts/SFNSMono.ttf", "/System/Library/Fonts/Menlo.ttc"],
            Kind::Hand => &["/System/Library/Fonts/Noteworthy.ttc", "/System/Library/Fonts/Supplemental/Bradley Hand Bold.ttf"],
            Kind::Emoji => &["/System/Library/Fonts/Apple Color Emoji.ttc", "/System/Library/Fonts/HelveticaNeue.ttc"],
        };
        return names.iter().map(PathBuf::from).collect();
    }
    let Some(dir) = std::env::var_os("WINDIR").map(|dir| PathBuf::from(dir).join("Fonts")) else {
        return Vec::new();
    };
    let names: &[&str] = match kind {
        Kind::Sans => &["seguisb.ttf", "segoeui.ttf", "arial.ttf"],
        Kind::Mono => &["consolab.ttf", "consola.ttf", "cour.ttf"],
        Kind::Hand => &["segoepr.ttf", "segoesc.ttf", "comic.ttf"],
        Kind::Emoji => &["seguiemj.ttf", "segoeui.ttf"],
    };
    names.iter().map(|name| dir.join(name)).collect()
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Sans,
    Mono,
    Hand,
    Emoji,
}
