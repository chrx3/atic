//! Los íconos de la pill según su estilo: Lucide en Atic y Material Symbols
//! (Rounded, peso 400, `assets/icons/ms/`) en Expressive, con la variante
//! rellena para la herramienta elegida. GPUI guarda cada SVG por su ruta, así
//! que cada estilo pide la suya en vez de cambiar lo que carga una misma ruta.
//! Lo que no está en la tabla sigue en Lucide en los dos estilos.

use crate::appearance::PillStyle;

macro_rules! material_symbols {
    ($($lucide:literal => $ms:literal,)*) => {
        /// La ruta del Material Symbol de un ícono de Lucide (`icons/x.svg`):
        /// el de contorno y el relleno.
        fn symbol(lucide: &str) -> Option<(&'static str, &'static str)> {
            // Los que comparten símbolo con otro, para no repetir su archivo.
            let lucide = match lucide {
                "icons/audio-lines.svg" => "icons/audio-waveform.svg",
                "icons/sliders.svg" => "icons/sliders-vertical.svg",
                "icons/code.svg" => "icons/code-xml.svg",
                other => other,
            };
            match lucide {
                $(concat!("icons/", $lucide, ".svg") => Some((
                    concat!("icons/ms/", $ms, ".svg"),
                    concat!("icons/ms/fill/", $ms, ".svg"),
                )),)*
                _ => None,
            }
        }

        /// Los bytes de `icons/ms/…`, para `Assets::load`.
        pub fn load(path: &str) -> Option<&'static [u8]> {
            match path {
                $(
                    concat!("icons/ms/", $ms, ".svg") => Some(include_bytes!(concat!("../assets/icons/ms/", $ms, ".svg"))),
                    concat!("icons/ms/fill/", $ms, ".svg") => Some(include_bytes!(concat!("../assets/icons/ms/fill/", $ms, ".svg"))),
                )*
                _ => None,
            }
        }
    };
}

material_symbols! {
    "audio-waveform" => "graphic_eq",
    "clipboard" => "content_paste",
    "notebook-pen" => "edit_note",
    "bot" => "smart_toy",
    "sliders-vertical" => "tune",
    "scan" => "screenshot_region",
    "brush" => "brush",
    "pipette" => "colorize",
    "notebook-tabs" => "auto_stories",
    "music-2" => "music_note",
    "smartphone" => "smartphone",
    "timer" => "timer",
    "mouse" => "mouse",
    "code-xml" => "code",
    "ellipsis" => "more_horiz",
    "chevron-left" => "chevron_left",
    "chevron-right" => "chevron_right",
    "chevron-up" => "keyboard_arrow_up",
    "chevron-down" => "keyboard_arrow_down",
    "layout-grid" => "grid_view",
    "settings-2" => "settings",
    "arrow-up-right" => "arrow_outward",
    "arrow-up" => "arrow_upward",
    "arrow-down" => "arrow_downward",
    "folder" => "folder",
    "folder-open" => "folder_open",
    "video" => "videocam",
    "play" => "play_arrow",
    "pause" => "pause",
    "rotate-ccw" => "replay",
    "rotate-cw" => "refresh",
    "skip-back" => "skip_previous",
    "skip-forward" => "skip_next",
    "plus" => "add",
    "minus" => "remove",
    "check" => "check",
    "x" => "close",
    "copy" => "content_copy",
    "pencil" => "edit",
    "laptop" => "laptop_mac",
    "monitor" => "monitor",
    "mouse-pointer-2" => "arrow_selector_tool",
    "sparkles" => "auto_awesome",
    "download" => "download",
    "circle-check" => "check_circle",
    "circle-x" => "cancel",
    "circle-help" => "help",
    "languages" => "translate",
    "sun" => "light_mode",
    "moon" => "dark_mode",
    "eye-off" => "visibility_off",
    "ruler" => "straighten",
    "mic" => "mic",
    "mic-off" => "mic_off",
    "volume-2" => "volume_up",
    "volume-1" => "volume_down",
    "volume-x" => "volume_off",
    "headphones" => "headphones",
    "search" => "search",
    "trash" => "delete",
    "history" => "history",
    "image" => "image",
    "star" => "star",
    "pin" => "keep",
    "lock" => "lock",
    "type" => "title",
    "undo" => "undo",
    "redo" => "redo",
    "external-link" => "open_in_new",
    "scan-text" => "document_scanner",
    "captions" => "closed_caption",
    "crop" => "crop",
    "highlighter" => "ink_highlighter",
    "eraser" => "ink_eraser",
    "file" => "description",
    "mail" => "mail",
    "scissors" => "content_cut",
    "cast" => "cast",
    "coffee" => "coffee",
    "power" => "power_settings_new",
    "square-terminal" => "terminal",
    "layers" => "layers",
    "gauge" => "speed",
    "cpu" => "memory",
    "thermometer" => "thermostat",
    "network" => "lan",
    "battery-charging" => "battery_charging_full",
    "battery-full" => "battery_full",
    "battery-medium" => "battery_4_bar",
    "battery-low" => "battery_1_bar",
}

/// La pill va en Material 3 Expressive.
pub fn expressive() -> bool {
    crate::appearance::current().pill == PillStyle::Expressive
}

/// El ícono `lucide` en el estilo de la pill.
pub fn themed(lucide: &'static str) -> &'static str {
    styled(lucide, expressive(), false)
}

/// `lucide` en Material Symbols si `expressive` (relleno si `filled`); si no,
/// o si no tiene equivalente, el de Lucide.
pub fn styled(lucide: &'static str, expressive: bool, filled: bool) -> &'static str {
    match symbol(lucide) {
        Some((outline, fill)) if expressive => if filled { fill } else { outline },
        _ => lucide,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cada_simbolo_se_carga_en_sus_dos_variantes() {
        for lucide in ["icons/mic.svg", "icons/audio-waveform.svg", "icons/play.svg", "icons/laptop.svg"] {
            let outline = styled(lucide, true, false);
            let fill = styled(lucide, true, true);
            assert!(outline.starts_with("icons/ms/") && load(outline).is_some(), "{outline}");
            assert!(fill.starts_with("icons/ms/fill/") && load(fill).is_some(), "{fill}");
        }
    }

    #[test]
    fn atic_y_los_sin_equivalente_siguen_en_lucide() {
        assert_eq!(styled("icons/mic.svg", false, true), "icons/mic.svg");
        assert_eq!(styled("icons/bubble.svg", true, false), "icons/bubble.svg");
    }
}
