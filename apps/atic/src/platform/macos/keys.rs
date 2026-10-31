//! El teclado en macOS con las teclas virtuales de Windows que usa el resto
//! de la pill (`dictation::Shortcut`): `Alt` es Opción, `Win` es Comando y
//! `CmdOrCtrl` también es Comando (lo arregla `Shortcut::parse`).
//!
//! Sondear con `CGEventSourceKeyState` pide el permiso de «Monitoreo de
//! entrada» para las teclas que no son modificadores (`permissions.rs` lo
//! pide al arrancar). Los modificadores salen de las banderas, que no lo piden.

use crate::dictation::{VK_XBUTTON1, VK_XBUTTON2};

/// `kCGEventSourceStateCombinedSessionState`.
const COMBINED: i32 = 0;

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventSourceKeyState(state: i32, key: u16) -> bool;
    fn CGEventSourceButtonState(state: i32, button: u32) -> bool;
    fn CGEventSourceFlagsState(state: i32) -> u64;
}

// `CGEventFlags`.
const FLAG_SHIFT: u64 = 0x0002_0000;
const FLAG_CONTROL: u64 = 0x0004_0000;
const FLAG_OPTION: u64 = 0x0008_0000;
const FLAG_COMMAND: u64 = 0x0010_0000;

/// Códigos de tecla de macOS (`kVK_*`) de las letras, en orden A–Z.
const LETTERS: [u16; 26] = [
    0x00, 0x0B, 0x08, 0x02, 0x0E, 0x03, 0x05, 0x04, 0x22, 0x26, 0x28, 0x25, 0x2E, 0x2D, 0x1F, 0x23, 0x0C, 0x0F,
    0x01, 0x11, 0x20, 0x09, 0x0D, 0x07, 0x10, 0x06,
];

/// Los dígitos de la fila de arriba, 0–9.
const DIGITS: [u16; 10] = [0x1D, 0x12, 0x13, 0x14, 0x15, 0x17, 0x16, 0x1A, 0x1C, 0x19];

/// F1–F20 (macOS no tiene más).
const FUNCTION: [u16; 20] = [
    0x7A, 0x78, 0x63, 0x76, 0x60, 0x61, 0x62, 0x64, 0x65, 0x6D, 0x67, 0x6F, 0x69, 0x6B, 0x71, 0x6A, 0x40, 0x4F,
    0x50, 0x5A,
];

pub const KEY_SPACE: u16 = 0x31;
pub const KEY_ESCAPE: u16 = 0x35;
pub const KEY_RETURN: u16 = 0x24;
pub const KEY_V: u16 = 0x09;

/// La tecla de macOS de una tecla virtual de Windows que no es modificador.
pub fn keycode(vk: i32) -> Option<u16> {
    match vk {
        0x20 => Some(KEY_SPACE),
        0x1B => Some(KEY_ESCAPE),
        0x0D => Some(KEY_RETURN),
        0x09 => Some(0x30),
        0x08 => Some(0x33),
        0x2E => Some(0x75),
        0x25 => Some(0x7B),
        0x26 => Some(0x7E),
        0x27 => Some(0x7C),
        0x28 => Some(0x7D),
        0x30..=0x39 => Some(DIGITS[(vk - 0x30) as usize]),
        0x41..=0x5A => Some(LETTERS[(vk - 0x41) as usize]),
        0x70..=0x83 => Some(FUNCTION[(vk - 0x70) as usize]),
        _ => None,
    }
}

/// La tecla virtual (de Windows) apretada ahora mismo, aunque el foco lo
/// tenga otra app.
pub fn vk_down(vk: i32) -> bool {
    // SAFETY: consultas puras del estado del teclado y el mouse.
    unsafe {
        let flags = || CGEventSourceFlagsState(COMBINED);
        match vk {
            0x10 => flags() & FLAG_SHIFT != 0,
            0x11 => flags() & FLAG_CONTROL != 0,
            0x12 => flags() & FLAG_OPTION != 0,
            // Comando: la izquierda (o cualquiera) y la derecha.
            0x5B => flags() & FLAG_COMMAND != 0,
            0x5C => CGEventSourceKeyState(COMBINED, 0x36),
            // Los botones laterales del mouse son el 3 y el 4 de Quartz.
            VK_XBUTTON1 => CGEventSourceButtonState(COMBINED, 3),
            VK_XBUTTON2 => CGEventSourceButtonState(COMBINED, 4),
            other => keycode(other).is_some_and(|key| CGEventSourceKeyState(COMBINED, key)),
        }
    }
}

/// Una tecla de macOS apretada ahora mismo.
pub fn key_down(keycode: u16) -> bool {
    // SAFETY: consulta pura.
    unsafe { CGEventSourceKeyState(COMBINED, keycode) }
}

/// Un atajo de `config.json` como se escribe en macOS: `CmdOrCtrl+Shift+4`
/// es «⇧⌘4», con los modificadores en el orden de Apple (⌃⌥⇧⌘).
pub fn pretty(text: &str) -> String {
    let mut mods = [false; 4];
    let mut keys = Vec::new();
    for part in text.split('+').map(str::trim).filter(|p| !p.is_empty()) {
        match part.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => mods[0] = true,
            "alt" | "option" => mods[1] = true,
            "shift" => mods[2] = true,
            "cmd" | "command" | "cmdorctrl" | "commandorcontrol" | "super" | "win" | "meta" => mods[3] = true,
            "space" => keys.push("Espacio".to_string()),
            "mousex1" => keys.push("Mouse 4".to_string()),
            "mousex2" => keys.push("Mouse 5".to_string()),
            other => keys.push(other.to_ascii_uppercase()),
        }
    }
    let mut out: String = ["⌃", "⌥", "⇧", "⌘"].iter().zip(mods).filter(|(_, on)| *on).map(|(s, _)| *s).collect();
    out.push_str(&keys.join("+"));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atajos_como_en_mac() {
        assert_eq!(pretty("CmdOrCtrl+Shift+4"), "⇧⌘4");
        assert_eq!(pretty("Alt+V"), "⌥V");
        assert_eq!(pretty("Ctrl+Alt+Space"), "⌃⌥Espacio");
    }

    #[test]
    fn letras_y_numeros_de_macos() {
        assert_eq!(keycode('A' as i32), Some(0x00));
        assert_eq!(keycode('Z' as i32), Some(0x06));
        assert_eq!(keycode('V' as i32), Some(KEY_V));
        assert_eq!(keycode('4' as i32), Some(0x15));
        assert_eq!(keycode(0x70), Some(0x7A));
        assert_eq!(keycode(0x20), Some(KEY_SPACE));
        assert_eq!(keycode(0x5B), None);
    }
}
